// Command vidlink-check resolves and probes real sources using the shared engine.
// All upstream traffic uses the locally attested Mullvad client. Private source
// URLs/context are written only to a mode-0600 temporary selection file.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"family-stream-providers/internal/provider"
	"family-stream-providers/internal/provider/vidcore"
	"family-stream-providers/internal/provider/vidlink"
	"family-stream-providers/internal/search"
)

func main() {
	if err := run(); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	mediaType := flag.String("type", "tv", "movie or tv")
	tmdbID := flag.Int64("tmdb", 60625, "canonical TMDB title ID")
	imdbID := flag.String("imdb", "", "optional IMDb cross-ID")
	season := flag.Int("season", 1, "TV season number")
	episode := flag.Int("episode", 1, "TV episode number")
	onlyVidLink := flag.Bool("vidlink-only", false, "omit VidCore for an isolated VidLink check")
	requireAudio := flag.String("require-audio", "", "require an established audio language")
	requireSubtitles := flag.String("require-subtitles", "", "require established playable subtitles")
	codecs := flag.String("codecs", "avc1,avc3,hvc1,hev1,mp4a,ac-3,ec-3", "comma-separated allowed codecs for the playback client")
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

	adapters := []provider.Adapter{vidlink.New(client)}
	if !*onlyVidLink {
		adapters = append(adapters, vidcore.New(client))
	}
	registry, err := provider.NewRegistry(adapters...)
	if err != nil {
		return fmt.Errorf("create provider registry: %w", err)
	}
	pool, err := probe.New(client, probe.DefaultLimits())
	if err != nil {
		return fmt.Errorf("create probe pool: %w", err)
	}
	engine, err := search.New(registry, pool, search.DefaultLimits(), nil)
	if err != nil {
		return fmt.Errorf("create selection engine: %w", err)
	}
	defer engine.Close()

	lookup := model.Lookup{
		Title: title,
		Device: model.Device{
			Formats:              []model.Format{model.HLS, model.MP4},
			Codecs:               strings.Split(*codecs, ","),
			MaxHeight:            2160,
			SupportsEncryptedHLS: true,
		},
	}
	if *requireAudio != "" {
		lookup.Preferences.AudioLanguages = []string{*requireAudio}
		lookup.Preferences.RequireAudio = true
	}
	if *requireSubtitles != "" {
		lookup.Preferences.SubtitleLanguages = []string{*requireSubtitles}
		lookup.Preferences.RequireSubtitles = true
	}

	selection, resolveErr := engine.Resolve(ctx, lookup)
	for _, status := range selection.Providers {
		fmt.Printf("Provider %s: candidates=%d failure=%s duration=%s\n",
			status.Provider, status.CandidateCount, status.Failure, status.Duration.Round(time.Millisecond))
	}
	if selection.Selected != nil {
		picked := selection.Selected.Candidate
		fmt.Printf("Selected %s: format=%s samples=%d backups=%d\n",
			picked.Source.Provider, picked.Source.Format, picked.Observation.SamplesCompleted, len(selection.Backups))
		if picked.Observation.Variant != nil && picked.Observation.Variant.Height != nil {
			fmt.Printf("Selected resolution: %dp\n", *picked.Observation.Variant.Height)
		}
	}
	fmt.Printf("Rejected candidates: %d; media content/playback not independently verified\n", len(selection.Rejected))

	encoded, err := json.MarshalIndent(selection, "", "  ")
	if err != nil {
		return fmt.Errorf("serialize private selection: %w", err)
	}
	directory, err := os.MkdirTemp("", "family-stream-vidlink-check-")
	if err != nil {
		return fmt.Errorf("create private selection directory: %w", err)
	}
	if err := os.WriteFile(filepath.Join(directory, "selection.json"), encoded, 0600); err != nil {
		return fmt.Errorf("write private selection: %w", err)
	}
	fmt.Println("Private selection directory:", directory)
	if resolveErr != nil {
		// Preserve the failure category without printing arbitrary nested errors.
		return fmt.Errorf("source selection ended with %s", model.Category(resolveErr))
	}
	return nil
}
