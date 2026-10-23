#!/bin/sh
#
#  streaming-entry.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Container entrypoint for the streaming service. Initializes runtime directories,
#  starts the privileged VPN owner first, waits for wg0 interface creation, and then
#  hands process supervision of providers and relay over to supervisord.
#
set -eu

# Initialize runtime and logging directories
mkdir -p /run/family-stream-vpn /run/supervisor /var/log/supervisor
chmod 755 /run/family-stream-vpn

# Start the privileged VPN owner process in the background.
# Source scraping processes remain held until firewall rules and wg0 are ready.
/usr/bin/python3 /opt/runtime/vpn-owner.py &
vpn_pid=$!

# Ensure the VPN process is terminated if startup fails early
trap 'kill -TERM "$vpn_pid" 2>/dev/null || true; wait "$vpn_pid" || true' EXIT

# Block until the WireGuard interface (wg0) is fully created and initialized
while [ ! -e /sys/class/net/wg0/ifindex ]; do
    kill -0 "$vpn_pid"
    sleep 1
done

# Launch supervisord to manage unprivileged provider and relay worker processes
/usr/bin/supervisord -n -c /opt/runtime/supervisord.ini &
supervisor_pid=$!

# Define graceful shutdown handler for container termination
stop() {
    kill -TERM "$supervisor_pid" 2>/dev/null || true
    wait "$supervisor_pid" || true
    kill -TERM "$vpn_pid" 2>/dev/null || true
    wait "$vpn_pid" || true
    exit 0
}
trap stop TERM INT

# Monitor health of both the VPN owner and supervisor daemon
while kill -0 "$vpn_pid" 2>/dev/null && kill -0 "$supervisor_pid" 2>/dev/null; do
    sleep 1 &
    wait $! || true
done

stop
