// Command vidcore-inspect downloads static embed HTML and JavaScript chunks from VidCore
// to a local directory for offline reverse-engineering and verification without code execution.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/provider/vidcore"
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

	outputDir := flag.String("output", "", "optional output directory to store captured files")
	mediaType := flag.String("type", "tv", "media type: movie or tv")
	tmdbID := flag.Int64("tmdb", 1622, "canonical TMDB title ID")
	imdbID := flag.String("imdb", "", "optional IMDb cross-ID")
	season := flag.Int("season", 1, "TV season number")
	episode := flag.Int("episode", 1, "TV episode number")
	flag.Parse()

	sampleTitle := model.Title{
		Type:   model.MediaType(*mediaType),
		TMDBID: *tmdbID,
		IMDbID: *imdbID,
	}
	if sampleTitle.Type == model.TV {
		sampleTitle.Season = season
		sampleTitle.Episode = episode
	}

	targetDir := *outputDir
	if targetDir == "" {
		tmpDir, err := os.MkdirTemp("", "family-stream-vidcore-")
		if err != nil {
			return fmt.Errorf("failed to create temporary capture directory: %w", err)
		}
		targetDir = tmpDir
	} else {
		if err := os.MkdirAll(targetDir, 0755); err != nil {
			return fmt.Errorf("failed to create target capture directory: %w", err)
		}
	}

	report, err := vidcore.Capture(ctx, client, sampleTitle, targetDir)
	if err != nil {
		return fmt.Errorf("vidcore capture failed: %w", err)
	}

	fmt.Printf(
		"Embed HTTP %d; %d chunks; private capture directory: %s\n",
		report.EmbedStatus,
		report.ChunkCount,
		targetDir,
	)

	for _, fileName := range report.MatchingFiles {
		fmt.Printf("Player structure confirmed in %s (downloaded code was not evaluated)\n", fileName)
	}

	return nil
}
