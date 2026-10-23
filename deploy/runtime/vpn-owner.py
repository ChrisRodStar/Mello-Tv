#!/usr/bin/python3
#
#  vpn-owner.py
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Privileged WireGuard manager and firewall owner inside the streaming container.
#  Configures fail-closed iptables rules before creating the wg0 tunnel interface,
#  publishes health attestation files, and enforces strict network isolation.
#
import configparser
import ipaddress
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

STATE_DIR = Path("/run/family-stream-vpn")
ATTESTATION_FILE = STATE_DIR / "attestation.json"
WIREGUARD_CONF = Path("/run/secrets/wireguard.conf")

stopping = False


def run_command(*args, capture=False):
    """Executes a system networking command with strict error handling and timeout."""
    return subprocess.run(
        args,
        check=True,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
        timeout=5,
    ).stdout


def signal_handler(signum, frame):
    """Handles SIGTERM and SIGINT for graceful shutdown."""
    global stopping
    stopping = True


def setup_fail_closed_firewall(endpoint_ip: str, endpoint_port: str):
    """
    Installs fail-closed kernel firewall rules before creating the tunnel interface.
    Defaults all traffic to DROP so that unprivileged services can never leak to the ISP.
    """
    # 1. Set default DROP policy across all chains for IPv4 and IPv6
    for tool in ["iptables", "ip6tables"]:
        for chain in ["INPUT", "OUTPUT", "FORWARD"]:
            run_command(tool, "-P", chain, "DROP")
            run_command(tool, "-F", chain)

        # Allow local loopback traffic
        for chain in ["INPUT", "OUTPUT"]:
            run_command(tool, "-A", chain, "-i" if chain == "INPUT" else "-o", "lo", "-j", "ACCEPT")

    # 2. Allow established connections
    run_command("iptables", "-A", "INPUT", "-m", "conntrack", "--ctstate", "ESTABLISHED,RELATED", "-j", "ACCEPT")

    # 3. Allow inbound RPC control from the API container (172.30.84.10) on ports 8093 & 8094
    run_command("iptables", "-A", "INPUT", "-i", "eth0", "-s", "172.30.84.10",
                "-p", "tcp", "-m", "multiport", "--dports", "8093,8094", "-j", "ACCEPT")

    # 4. Allow inbound stream requests from host bridge gateway (172.30.84.1) on port 8095
    run_command("iptables", "-A", "INPUT", "-i", "eth0", "-s", "172.30.84.1",
                "-p", "tcp", "--dport", "8095", "-j", "ACCEPT")

    # 5. Allow outbound TCP replies to private bridge clients (172.30.84.0/24)
    run_command("iptables", "-A", "OUTPUT", "-o", "eth0", "-d", "172.30.84.0/24",
                "-p", "tcp", "-m", "multiport", "--sports", "8093,8094,8095",
                "-m", "conntrack", "--ctstate", "ESTABLISHED", "-j", "ACCEPT")

    # 6. Allow marked WireGuard UDP traffic to reach the Mullvad endpoint via eth0
    run_command("iptables", "-A", "OUTPUT", "-o", "eth0", "-d", endpoint_ip,
                "-p", "udp", "--dport", endpoint_port, "-m", "mark", "--mark", "51820", "-j", "ACCEPT")

    # 7. Allow all outbound traffic through the WireGuard interface (wg0)
    run_command("iptables", "-A", "OUTPUT", "-o", "wg0", "-j", "ACCEPT")


def setup_wireguard_interface(interface_config, peer_config, endpoint_ip: str, endpoint_port: str):
    """Configures the WireGuard interface and sets routing policies."""
    # Ensure physical route to the VPN endpoint via the physical Docker gateway
    default_route_info = json.loads(run_command("ip", "-j", "-4", "route", "show", "default", capture=True))[0]
    run_command("ip", "route", "replace", f"{endpoint_ip}/32", "via", default_route_info["gateway"], "dev", "eth0")

    # Create the wg0 tunnel interface
    run_command("ip", "link", "add", "wg0", "type", "wireguard")

    # Prepare temporary WireGuard configuration file
    wg_temp_config = STATE_DIR / "wireguard.setconf"
    config_lines = [
        "[Interface]",
        f"PrivateKey = {interface_config['PrivateKey']}",
        "FwMark = 51820",
        "[Peer]",
    ]
    for key in ["PublicKey", "PresharedKey", "AllowedIPs", "Endpoint"]:
        if key in peer_config:
            config_lines.append(f"{key} = {peer_config[key]}")
    config_lines.append("PersistentKeepalive = 25\n")

    # Write config with restricted permissions
    fd = os.open(wg_temp_config, os.O_CREAT | os.O_WRONLY | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write("\n".join(config_lines))

    try:
        run_command("wg", "setconf", "wg0", str(wg_temp_config))
    finally:
        wg_temp_config.unlink(missing_ok=True)

    # Assign client tunnel IP address
    for address in interface_config["Address"].split(","):
        parsed_address = ipaddress.ip_interface(address.strip())
        if parsed_address.version == 4:
            run_command("ip", "address", "add", str(parsed_address), "dev", "wg0")

    # Set MTU and bring interface UP
    run_command("ip", "link", "set", "wg0", "mtu", "1280", "up")

    # Route default outbound traffic through wg0
    run_command("ip", "route", "replace", "default", "dev", "wg0")


def monitor_tunnel_health(endpoint_ip: str, endpoint_port: str):
    """
    Continuous monitoring loop that inspects tunnel liveness, handshakes, and routes.
    Publishes an atomic attestation file only when the VPN is active and healthy.
    """
    while not stopping:
        try:
            # 1. Verify latest handshake is fresh (< 180 seconds)
            handshakes = run_command("wg", "show", "wg0", "latest-handshakes", capture=True).splitlines()
            if len(handshakes) != 1:
                raise ValueError("Multiple peers or no peer configured")

            public_key, timestamp_str = handshakes[0].split()
            handshake_age = time.time() - int(timestamp_str)
            if not (0 <= handshake_age < 180):
                raise ValueError(f"Handshake is stale ({handshake_age:.0f}s old)")

            # 2. Verify default route points to wg0
            current_route = json.loads(run_command("ip", "-j", "-4", "route", "show", "default", capture=True))
            if len(current_route) != 1 or current_route[0].get("dev") != "wg0":
                raise ValueError("Default route is not bound to wg0")

            # 3. Verify interface UP flag
            flags_hex = Path("/sys/class/net/wg0/flags").read_text().strip()
            if int(flags_hex, 16) & 1 == 0:
                raise ValueError("wg0 interface is DOWN")

            # 4. Publish healthy attestation file atomically
            if_index = socket.if_nametoindex("wg0")
            attestation_data = {
                "interface": "wg0",
                "index": if_index,
                "identity": f"{if_index}|{public_key}|{endpoint_ip}:{endpoint_port}",
            }
            temp_attest = STATE_DIR / "attestation.new"
            temp_attest.write_text(json.dumps(attestation_data, indent=2) + "\n")
            temp_attest.chmod(0o644)
            temp_attest.replace(ATTESTATION_FILE)

        except (ValueError, OSError, subprocess.SubprocessError, KeyError):
            # Tunnel is degraded or disconnected: revoke attestation immediately
            ATTESTATION_FILE.unlink(missing_ok=True)

        time.sleep(0.5)


def main():
    STATE_DIR.mkdir(mode=0o755, exist_ok=True)
    ATTESTATION_FILE.unlink(missing_ok=True)

    # Parse WireGuard configuration
    config = configparser.ConfigParser(interpolation=None)
    config.read(str(WIREGUARD_CONF))
    interface_config = config["Interface"]
    peer_config = config["Peer"]

    raw_endpoint, raw_port = peer_config["Endpoint"].rsplit(":", 1)
    endpoint_ip = str(ipaddress.IPv4Address(raw_endpoint))
    endpoint_port = str(int(raw_port))

    # 1. Install fail-closed firewall rules first
    setup_fail_closed_firewall(endpoint_ip, endpoint_port)

    # 2. Create and initialize WireGuard tunnel
    setup_wireguard_interface(interface_config, peer_config, endpoint_ip, endpoint_port)

    # 3. Enter health monitoring loop
    monitor_tunnel_health(endpoint_ip, endpoint_port)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)
    try:
        main()
    finally:
        ATTESTATION_FILE.unlink(missing_ok=True)
        # Note: The DROP firewall is deliberately retained through shutdown/failure.
