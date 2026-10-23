package network

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"sync"
	"time"
)

// LocalMullvadOptions establishes platform-specific fail-closed sockets bound to the active Mullvad tunnel.
//
// Pipeline:
//  1. Attest the platform VPN owner and locate its active tunnel interface.
//  2. Install a continuous guard callback that verifies the interface remains active and unchanged.
//  3. Bind all outbound sockets using the platform interface-binding option.
//  4. Route DNS resolution via tunnel-bound DNS-over-HTTPS (DoH) to prevent macOS loopback leaks or physical fallback.
func LocalMullvadOptions(ctx context.Context, dnsAddress string) (Options, error) {
	return LocalMullvadOptionsWithBudget(ctx, dnsAddress, nil)
}

// LocalMullvadOptionsWithBudget applies the independent Go acceptance allowance
// to both provider/probe HTTP and tunnel-bound DoH requests.
func LocalMullvadOptionsWithBudget(ctx context.Context, dnsAddress string, budget *AcceptanceBudget) (Options, error) {
	interfaceName, err := mullvadTunnel(ctx)
	if err != nil {
		return Options{}, err
	}

	iface, err := net.InterfaceByName(interfaceName)
	if err != nil {
		return Options{}, errors.New("tunnel interface missing")
	}

	dnsHost, dnsPort, err := net.SplitHostPort(dnsAddress)
	parsedDNS, addressErr := netip.ParseAddr(dnsHost)
	if err != nil || addressErr != nil || !PublicIP(parsedDNS) || dnsPort != "53" {
		return Options{}, errors.New("literal tunnel DNS address required")
	}

	guard := GuardFunc(func(guardCtx context.Context) error {
		if err := budget.WorkCheck(); err != nil {
			return err
		}
		currentName, err := mullvadTunnel(guardCtx)
		if err != nil || currentName != interfaceName {
			return errors.New("mullvad tunnel changed or unavailable")
		}

		actualIface, err := net.InterfaceByName(interfaceName)
		if err != nil || actualIface.Index != iface.Index {
			return errors.New("mullvad interface changed")
		}

		return nil
	})

	socketControl := boundControl(iface.Index)

	// DNS-over-HTTPS transport bound to the tunnel interface.
	dohTransport := &http.Transport{
		Proxy:                  nil,
		DisableCompression:     true,
		MaxResponseHeaderBytes: 16 * 1024,
		DialContext: func(dialCtx context.Context, networkProto, address string) (net.Conn, error) {
			if err := guard.Check(dialCtx); err != nil {
				return nil, err
			}
			dialer := net.Dialer{
				Timeout: 2 * time.Second,
				Control: socketControl,
			}
			return dialer.DialContext(dialCtx, "tcp", net.JoinHostPort(dnsHost, "443"))
		},
		MaxConnsPerHost:     4,
		TLSHandshakeTimeout: 2 * time.Second,
	}

	dohClient := &http.Client{
		Transport: dohTransport,
		Timeout:   3 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}

	ownerCtx, cancelOwner := context.WithCancel(context.Background())
	var bridgeMu sync.Mutex
	var bridgeWg sync.WaitGroup
	var closed bool

	// In-process RFC 1035 / RFC 8484 DNS bridge connecting net.Resolver to tunnel-bound DoH.
	customResolver := &net.Resolver{
		PreferGo: true,
		Dial: func(resolverCtx context.Context, networkProto, address string) (net.Conn, error) {
			if err := guard.Check(resolverCtx); err != nil {
				return nil, err
			}

			bridgeMu.Lock()
			if closed {
				bridgeMu.Unlock()
				return nil, errors.New("DNS bridge closed")
			}
			bridgeWg.Add(1)
			bridgeMu.Unlock()
			clientPipe, bridgePipe := net.Pipe()

			go func() {
				defer bridgeWg.Done()
				resolverCtx, cancelBridge := context.WithCancel(resolverCtx)
				stopOwner := context.AfterFunc(ownerCtx, cancelBridge)
				defer stopOwner()
				defer cancelBridge()
				resolverCtx, stopBudget := budget.Context(resolverCtx)
				defer stopBudget()
				defer func() {
					_ = bridgePipe.Close()
				}()

				cancelOnTimeout := context.AfterFunc(resolverCtx, func() {
					_ = bridgePipe.Close()
				})
				defer cancelOnTimeout()

				// Read 2-byte query length prefix (TCP DNS).
				var lengthPrefix [2]byte
				if _, err := io.ReadFull(bridgePipe, lengthPrefix[:]); err != nil {
					return
				}

				queryLen := int(binary.BigEndian.Uint16(lengthPrefix[:]))
				if queryLen < 12 {
					return
				}

				dnsQuery := make([]byte, queryLen)
				if _, err := io.ReadFull(bridgePipe, dnsQuery); err != nil {
					return
				}

				if err := guard.Check(resolverCtx); err != nil {
					return
				}

				dohReq, err := http.NewRequestWithContext(
					resolverCtx,
					http.MethodPost,
					"https://cloudflare-dns.com/dns-query",
					bytes.NewReader(dnsQuery),
				)
				if err != nil {
					return
				}

				dohReq.Header.Set("Content-Type", "application/dns-message")
				dohReq.Header.Set("Accept", "application/dns-message")

				if budget.Take(65536+16*1024+32*1024) != nil {
					return
				}
				dohResp, err := dohClient.Do(dohReq)
				if err != nil {
					return
				}
				defer func() {
					_ = dohResp.Body.Close()
				}()

				if dohResp.StatusCode != http.StatusOK {
					return
				}

				respBody, err := io.ReadAll(io.LimitReader(dohResp.Body, 65536))
				if err != nil || len(respBody) < 12 || len(respBody) > 65535 {
					return
				}

				binary.BigEndian.PutUint16(lengthPrefix[:], uint16(len(respBody)))
				_, _ = bridgePipe.Write(append(lengthPrefix[:], respBody...))
			}()

			return clientPipe, nil
		},
	}

	return Options{
		Budget:        budget,
		Guard:         guard,
		Resolver:      customResolver,
		SocketControl: socketControl,
		Cleanup: func() {
			bridgeMu.Lock()
			closed = true
			bridgeMu.Unlock()
			cancelOwner()
			dohClient.CloseIdleConnections()
			bridgeWg.Wait()
		},
	}, nil
}
