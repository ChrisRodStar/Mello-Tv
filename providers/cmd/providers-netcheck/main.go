// Command providers-netcheck attests both Go source address families through
// the exact production guarded client and independent Go acceptance allowance.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"time"

	"stv-providers/internal/network"
)

func main() {
	if err := run(); err != nil {
		// run returns only fixed categories, never wrapped transport errors/URLs.
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	if len(os.Args) != 2 {
		return errors.New("private acceptance ledger required")
	}
	budget, err := network.OpenAcceptanceBudget(os.Args[1])
	if err != nil {
		return errors.New("acceptance allowance unavailable")
	}
	defer budget.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	for _, family := range []string{
		"ip4",
		"ip6",
	} {
		if err := attest(ctx, budget, family); err != nil {
			return fmt.Errorf("%s: %w", family, err)
		}
		fmt.Printf("%s: guarded_transport_mullvad_exit=true\n", family)
	}
	fmt.Println("Go guarded Mullvad egress attested: IPv4 and IPv6; DNS remains tunnel-bound")
	return nil
}

func attest(ctx context.Context, budget *network.AcceptanceBudget, family string) error {
	endpoint, err := attestationEndpoint(family)
	if err != nil {
		return err
	}
	options, err := network.LocalMullvadOptionsWithBudget(ctx, "1.1.1.1:53", budget)
	if err != nil {
		return errors.New("guard setup unavailable")
	}
	options.AddressFamily = family
	client, err := network.New(options)
	if err != nil {
		if options.Cleanup != nil {
			options.Cleanup()
		}
		return errors.New("guarded client unavailable")
	}
	defer client.Close()
	response, err := client.Fetch(ctx, network.Request{
		URL:   endpoint,
		Limit: 16 * 1024,
	})
	if err != nil {
		return errors.New("attestation request rejected")
	}
	return verifyAttestation(response)
}

func attestationEndpoint(family string) (string, error) {
	// Match the relay's explicit family-specific diagnostics. Forcing ip6 on
	// a generic hostname does not establish that it publishes an AAAA answer.
	switch family {
	case "ip4":
		return "https://ipv4.am.i.mullvad.net/json", nil
	case "ip6":
		return "https://ipv6.am.i.mullvad.net/json", nil
	default:
		return "", errors.New("invalid attestation family")
	}
}

func verifyAttestation(response network.Response) error {
	var result struct {
		Mullvad bool `json:"mullvad_exit_ip"`
	}
	if response.Status != http.StatusOK || json.Unmarshal(response.Body, &result) != nil || !result.Mullvad {
		return errors.New("egress not attested")
	}
	return nil
}
