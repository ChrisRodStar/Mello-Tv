//go:build !darwin && !linux

package network

import (
	"context"
	"errors"
	"syscall"
)

func mullvadTunnel(context.Context) (string, error) {
	return "", errors.New("guarded tunnel unsupported on this platform")
}

func boundControl(int) func(string, string, syscall.RawConn) error {
	return func(string, string, syscall.RawConn) error {
		return errors.New("guarded socket unsupported on this platform")
	}
}
