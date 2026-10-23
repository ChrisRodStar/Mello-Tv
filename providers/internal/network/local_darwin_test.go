//go:build darwin

package network

import (
	"context"
	"net"
	"testing"
	"time"
)

func TestMissingTunnelInterfaceFailsBeforeConnecting(t *testing.T) {
	// A nonexistent interface fails socket setup; no outbound connection can occur.
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	d := net.Dialer{Control: boundControl(2147483647)}
	conn, e := d.DialContext(ctx, "tcp4", "93.184.216.34:443")
	if conn != nil {
		_ = conn.Close()
	}
	if e == nil {
		t.Fatal("missing tunnel binding allowed a connection")
	}
}
