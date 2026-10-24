// Command providers runs the private, bounded providers.v1 HTTP service.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/netip"
	"os"
	"os/signal"
	"strings"
	"syscall"

	"stv-providers/internal/config"
	"stv-providers/internal/network"
	httptransport "stv-providers/internal/transport/http"
)

type settings struct {
	TrustedListenIP      string `json:"trusted_listen_ip,omitempty"`
	AcceptanceBudgetFile string `json:"acceptance_budget_file,omitempty"`
	Listen               string `json:"listen"`
	BearerFile           string `json:"bearer_file"`
	DNSAddress           string `json:"dns_address"`
}

func main() {
	if err := run(); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "providers stopped: %v\n", err)
		os.Exit(1)
	}
}

func run() error {
	if len(os.Args) != 2 {
		return errors.New("one private config file required")
	}
	encoded, err := readPrivate(os.Args[1], 8192)
	if err != nil {
		return err
	}
	decoder := json.NewDecoder(strings.NewReader(string(encoded)))
	decoder.DisallowUnknownFields()
	var options settings
	if decoder.Decode(&options) != nil || decoder.Decode(new(any)) != io.EOF {
		return errors.New("invalid configuration")
	}
	address, err := trustedListenAddress(options.Listen, options.TrustedListenIP)
	if err != nil {
		return errors.New("loopback listener required")
	}
	if options.DNSAddress == "" {
		options.DNSAddress = "1.1.1.1:53"
	}
	bearer, err := readPrivate(options.BearerFile, 4096)
	if err != nil {
		return err
	}
	var budget *network.AcceptanceBudget
	if options.AcceptanceBudgetFile != "" {
		budget, err = network.OpenAcceptanceBudget(options.AcceptanceBudgetFile)
		if err != nil {
			return err
		}
		defer budget.Close()
	}
	engine := config.NewService(options.DNSAddress, budget)
	defer engine.Close()
	handler, err := httptransport.New(engine, bearer)
	if err != nil {
		return err
	}
	listener, err := net.ListenTCP("tcp", address)
	if err != nil {
		return errors.New("listener unavailable")
	}
	defer listener.Close()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	return httptransport.Serve(ctx, listener, handler)
}

// Parse literals before binding; invalid hostnames must never invoke system DNS.
func listenAddress(value string) (*net.TCPAddr, error) {
	return trustedListenAddress(value, "")
}

func trustedListenAddress(value, trusted string) (*net.TCPAddr, error) {
	address, err := netip.ParseAddrPort(value)
	trustedIP, _ := netip.ParseAddr(trusted)
	trustedPrivate := address.Addr().IsPrivate() && address.Addr() == trustedIP
	if err != nil || (!address.Addr().Unmap().IsLoopback() && !trustedPrivate) || address.Addr().Zone() != "" || address.Port() == 0 {
		return nil, errors.New("literal loopback listener required")
	}
	return &net.TCPAddr{
		IP:   net.IP(address.Addr().AsSlice()),
		Port: int(address.Port()),
	}, nil
}

func readPrivate(path string, limit int64) ([]byte, error) {
	file, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if err != nil {
		return nil, errors.New("private file unavailable")
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 {
		return nil, errors.New("private file mode required")
	}
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil || len(data) == 0 || int64(len(data)) > limit {
		return nil, errors.New("private file bound")
	}
	return data, nil
}
