// Command videasy-check resolves and probes real Videasy sources using the shared engine.
// All upstream traffic uses the locally attested Mullvad client.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/probe"
	"stv-providers/internal/provider"
	"stv-providers/internal/provider/vidcore"
	"stv-providers/internal/provider/videasy"
	"stv-providers/internal/provider/vidfast"
	"stv-providers/internal/provider/vidlink"
	"stv-providers/internal/provider/vidsrc"
	"stv-providers/internal/search"
)

func main() {
	if err := run(); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	mediaType := flag.String("type", "tv", "movie or tv")
	tmdbID := flag.Int64("tmdb", 1622, "canonical TMDB title ID (default: 1622 for Supernatural)")
	imdbID := flag.String("imdb", "tt0460681", "optional IMDb cross-ID")
	season := flag.Int("season", 1, "TV season number")
	episode := flag.Int("episode", 10, "TV episode number")
	onlyVideasy := flag.Bool("videasy-only", false, "omit other providers for an isolated Videasy check")
	flag.Parse()

	title := model.Title{
		Type:   model.MediaType(*mediaType),
		TMDBID: *tmdbID,
		IMDbID: *imdbID,
	}
	if title.Type == model.TV {
		title.Season = season
		title.Episode = episode
	}
	if err := title.Validate(); err != nil {
		return fmt.Errorf("validate lookup: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()

	options, err := network.LocalMullvadOptions(ctx, "1.1.1.1:53")
	if err != nil {
		return fmt.Errorf("configure local Mullvad client: %w", err)
	}
	client, err := network.New(options)
	if err != nil {
		return fmt.Errorf("create controlled client: %w", err)
	}
	defer client.Close()

	egress, err := client.Fetch(ctx, network.Request{
		URL:   "https://am.i.mullvad.net/json",
		Limit: 16 * 1024,
	})
	if err != nil {
		return fmt.Errorf("Mullvad egress request rejected")
	}
	var attestation struct {
		Mullvad bool `json:"mullvad_exit_ip"`
	}
	if egress.Status != http.StatusOK || json.Unmarshal(egress.Body, &attestation) != nil || !attestation.Mullvad {
		return fmt.Errorf("Mullvad egress not verified")
	}
	fmt.Println("Mullvad egress verified through tunnel-bound provider and DNS sockets")

	var adapters []provider.Adapter
	if *onlyVideasy {
		adapters = []provider.Adapter{videasy.New(client)}
	} else {
		adapters = []provider.Adapter{
			vidcore.New(client),
			vidlink.New(client),
			vidsrc.New(client),
			vidfast.New(client),
			videasy.New(client),
		}
	}

	registry, err := provider.NewRegistry(adapters...)
	if err != nil {
		return fmt.Errorf("create provider registry: %w", err)
	}

	prober, err := probe.New(client, probe.DefaultLimits())
	if err != nil {
		return fmt.Errorf("create probe pool: %w", err)
	}

	engine, err := search.New(registry, prober, search.DefaultLimits(), nil)
	if err != nil {
		return fmt.Errorf("create selection engine: %w", err)
	}
	defer engine.Close()

	lookup := model.Lookup{
		Title: title,
		Device: model.Device{
			Formats:              []model.Format{model.HLS, model.MP4},
			Codecs:               []string{"avc1", "avc3", "hvc1", "hev1", "mp4a", "ac-3", "ec-3"},
			MaxHeight:            2160,
			SupportsEncryptedHLS: true,
		},
	}

	fmt.Printf("Searching providers for TMDB %d (Type: %s)...\n", *tmdbID, *mediaType)
	selection, resolveErr := engine.Resolve(ctx, lookup)

	fmt.Println("\n--- Provider Race & Resolution Speeds ---")
	for _, status := range selection.Providers {
		fmt.Printf("Provider %-8s: candidates=%-2d duration=%-8s failure=%s\n",
			status.Provider, status.CandidateCount, status.Duration.Round(time.Millisecond), status.Failure)
	}

	if selection.Selected != nil {
		picked := selection.Selected.Candidate
		fmt.Printf("\n--- Winning Candidate ---\n")
		fmt.Printf("Selected Provider:   %s (%s)\n", picked.Source.Provider, picked.Source.Server)
		fmt.Printf("Selected Format:     %s\n", picked.Source.Format)
		if picked.Observation.Variant != nil && picked.Observation.Variant.Height != nil {
			fmt.Printf("Selected Resolution: %dp\n", *picked.Observation.Variant.Height)
		}
		if picked.Observation.Variant != nil && picked.Observation.Variant.Bitrate != nil {
			fmt.Printf("Variant Bitrate:     %.2f Mbps\n", float64(*picked.Observation.Variant.Bitrate)/1_000_000.0)
		}
		if picked.Observation.DeliveryBPS != nil {
			fmt.Printf("Delivery Speed:      %.2f Mbps\n", *picked.Observation.DeliveryBPS/1_000_000.0)
		}
		if picked.Observation.FirstByte != nil {
			fmt.Printf("Time to First Byte:  %v\n", picked.Observation.FirstByte.Round(time.Millisecond))
		}
		fmt.Printf("Samples Completed:   %d\n", picked.Observation.SamplesCompleted)
		fmt.Printf("Backup Candidates:   %d\n", len(selection.Backups))
		fmt.Printf("Stream URL:          %s\n", picked.Source.URL)
	}

	fmt.Printf("Total candidates: %d, rejected: %d\n",
		len(selection.Backups)+1, len(selection.Rejected))

	for id, reason := range selection.Rejected {
		fmt.Printf("  Rejected ID=%s Reason=%s\n", id, reason)
	}

	encoded, err := json.MarshalIndent(selection, "", "  ")
	if err != nil {
		return fmt.Errorf("serialize selection: %w", err)
	}

	dir, err := os.MkdirTemp("", "videasy-check-")
	if err == nil {
		_ = os.WriteFile(filepath.Join(dir, "selection.json"), encoded, 0600)
		fmt.Printf("Selection artifact written to: %s/selection.json\n", dir)
	}

	if resolveErr != nil {
		return fmt.Errorf("source selection ended with %s", model.Category(resolveErr))
	}

	return nil
}
