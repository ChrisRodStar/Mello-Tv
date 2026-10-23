// Command vidcore-inspect downloads static embed HTML and JavaScript chunks from VidCore
// to a local directory for offline reverse-engineering and verification without code execution.
package main

import (
	"context"
	"encoding/json"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/provider/vidcore"
	"fmt"
	"os"
	"time"
)

func main() {
	if err := run(); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "Error:", err)
		os.Exit(1)
	}
}

func run() error {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()

	opts, err := network.LocalMullvadOptions(ctx, "1.1.1.1:53")
	if err != nil {
		return fmt.Errorf("local VPN setup failed: %w", err)
	}

	client, err := network.New(opts)
	if err != nil {
		return fmt.Errorf("network client initialization failed: %w", err)
	}
	defer client.Close()

	// Verify egress traffic is properly routing through Mullvad VPN.
	checkResp, err := client.Fetch(ctx, network.Request{
		URL:   "https://am.i.mullvad.net/json",
		Limit: 16384,
	})
	if err != nil {
		return fmt.Errorf("mullvad egress check failed: %w", err)
	}

	var egressStatus struct {
		Mullvad bool `json:"mullvad_exit_ip"`
	}
	if err := json.Unmarshal(checkResp.Body, &egressStatus); err != nil || !egressStatus.Mullvad {
		return fmt.Errorf("mullvad egress verification failed: connection is not routing through Mullvad exit IP")
	}

	fmt.Println("Mullvad egress verified: provider and DNS sockets are securely bound to the tunnel")

	tmpDir, err := os.MkdirTemp("", "family-stream-vidcore-")
	if err != nil {
		return fmt.Errorf("failed to create temporary capture directory: %w", err)
	}

	season, episode := 1, 1
	sampleTitle := model.Title{
		Type:    model.TV,
		TMDBID:  60625,
		IMDbID:  "tt2861424",
		Season:  &season,
		Episode: &episode,
	}

	report, err := vidcore.Capture(ctx, client, sampleTitle, tmpDir)
	if err != nil {
		return fmt.Errorf("capture failed: %w", err)
	}

	fmt.Printf(
		"Embed HTTP %d; %d chunks; private capture directory: %s\n",
		report.EmbedStatus,
		report.ChunkCount,
		tmpDir,
	)

	for _, fileName := range report.MatchingFiles {
		fmt.Printf("Player structure confirmed in %s (downloaded code was not evaluated)\n", fileName)
	}

	return nil
}
