//go:build linux

package network

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"os"
	"syscall"
	"time"
)

// The privileged container VPN owner publishes this short-lived, root-owned
// attestation only while firewall, interface, route, and WireGuard handshake agree.
// Source processes cannot write the directory or modify the network namespace.
func mullvadTunnel(ctx context.Context) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	info, err := os.Lstat("/run/family-stream-vpn/attestation.json")
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0022 != 0 || time.Since(info.ModTime()) > 2*time.Second || time.Until(info.ModTime()) > time.Second {
		return "", errors.New("VPN attestation unavailable")
	}
	owner, ok := info.Sys().(*syscall.Stat_t)
	if !ok || owner.Uid != 0 {
		return "", errors.New("VPN attestation owner invalid")
	}
	encoded, err := os.ReadFile("/run/family-stream-vpn/attestation.json")
	if err != nil || len(encoded) > 4096 {
		return "", errors.New("VPN attestation unavailable")
	}
	var state struct {
		Interface string `json:"interface"`
		Index     int    `json:"index"`
	}
	if json.Unmarshal(encoded, &state) != nil || state.Interface != "wg0" {
		return "", errors.New("VPN attestation invalid")
	}
	actual, err := net.InterfaceByName(state.Interface)
	if err != nil || actual.Index != state.Index || actual.Flags&net.FlagUp == 0 {
		return "", errors.New("VPN interface unavailable")
	}
	return state.Interface, nil
}

func boundControl(interfaceIndex int) func(string, string, syscall.RawConn) error {
	return func(networkProto, address string, raw syscall.RawConn) error {
		if interfaceIndex <= 0 {
			return errors.New("invalid tunnel interface")
		}
		actual, err := net.InterfaceByIndex(interfaceIndex)
		if err != nil || actual.Name != "wg0" || actual.Flags&net.FlagUp == 0 {
			return errors.New("VPN interface unavailable")
		}
		var bindErr error
		if err := raw.Control(func(fd uintptr) {
			bindErr = syscall.SetsockoptString(int(fd), syscall.SOL_SOCKET, syscall.SO_BINDTODEVICE, actual.Name)
		}); err != nil {
			return errors.New("VPN socket control unavailable")
		}
		if bindErr != nil {
			return errors.New("VPN socket binding unavailable")
		}
		return nil
	}
}
