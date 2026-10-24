//go:build darwin

package network

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"os"
	"os/exec"
	"strings"
	"syscall"
	"time"
)

// localTunnel holds the JSON state returned by `/usr/local/bin/mullvad status --json`.
type localTunnel struct {
	State   string `json:"state"`
	Details struct {
		Endpoint struct {
			Interface string `json:"tunnel_interface"`
		} `json:"endpoint"`
	} `json:"details"`
}

// mullvadTunnel queries the Mullvad CLI for the active tunnel interface name (e.g. "utun3").
func mullvadTunnel(ctx context.Context) (string, error) {
	checkCtx, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()

	binPath := "/usr/local/bin/mullvad"
	if _, err := os.Stat(binPath); err != nil {
		if p, err := exec.LookPath("mullvad"); err == nil {
			binPath = p
		}
	}

	cmd := exec.CommandContext(checkCtx, binPath, "status", "--json")
	output, err := cmd.Output()
	if err != nil {
		return "", errors.New("local Mullvad tunnel unavailable")
	}

	var state localTunnel
	if err := json.Unmarshal(output, &state); err != nil || state.State != "connected" || !strings.HasPrefix(state.Details.Endpoint.Interface, "utun") {
		return "", errors.New("local Mullvad tunnel unavailable")
	}

	return state.Details.Endpoint.Interface, nil
}

// boundControl returns a socket control closure that binds sockets directly to the specified Darwin interface.
func boundControl(interfaceIndex int) func(string, string, syscall.RawConn) error {
	return func(networkProto, address string, raw syscall.RawConn) error {
		if interfaceIndex <= 0 {
			return errors.New("invalid tunnel interface")
		}

		var bindErr error
		err := raw.Control(func(fd uintptr) {
			// Darwin IP_BOUND_IF (25) and IPV6_BOUND_IF (125) bind the socket directly to this interface index.
			level := syscall.IPPROTO_IP
			option := 25

			host, _, err := net.SplitHostPort(address)
			if err != nil {
				host = address
			}
			if strings.HasSuffix(networkProto, "6") || strings.Contains(host, ":") {
				level = syscall.IPPROTO_IPV6
				option = 125
			}

			bindErr = syscall.SetsockoptInt(int(fd), level, option, interfaceIndex)
		})

		if err != nil || bindErr != nil {
			return errors.New("tunnel socket binding failed")
		}

		return nil
	}
}
