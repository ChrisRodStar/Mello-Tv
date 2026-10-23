// Command vidcore-check tests the VidCore resolver against live upstream endpoints,
// dumps resolved source metadata, probes media samples, and runs the ranking engine.
package main

import (
	"context"
	"encoding/json"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"family-stream-providers/internal/provider/vidcore"
	"family-stream-providers/internal/selection"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

func main() {
	if err := run(); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "Error:", err)
		os.Exit(1)
	}
}

func run() error {
	requireAudio := flag.String("require-audio", "", "require a reported audio language, such as en")
	requireSubtitles := flag.String("require-subtitles", "", "require a reported subtitle language, such as en")
	contextCheck := flag.Bool("context-check", false, "compare reference public CDN headers on a failed continuity sample")
	continuity := flag.Int("continuity", 0, "optional consecutive startup segment checks (maximum 24 per rendition)")
	mediaType := flag.String("type", "tv", "media type: movie or tv")
	tmdbID := flag.Int64("tmdb", 60625, "canonical TMDB title ID")
	imdbID := flag.String("imdb", "", "optional IMDb cross-ID")
	season := flag.Int("season", 1, "TV season number")
	episode := flag.Int("episode", 1, "TV episode number")
	flag.Parse()

	if *continuity < 0 || *continuity > 24 {
		return fmt.Errorf("continuity sample count must be between 0 and 24")
	}

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
		return fmt.Errorf("invalid lookup: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
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

	lookup := model.Lookup{
		Title: title,
		Device: model.Device{
			Formats:                []model.Format{model.HLS},
			Codecs:                 []string{"avc1", "hvc1", "hev1", "mp4a", "ac-3", "ec-3"},
			MaxHeight:              2160,
			AllowUnknownCodecs:     true,
			AllowUnknownResolution: true,
			SupportsEncryptedHLS:   true,
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

	startTime := time.Now()
	adapter := vidcore.NewWithSchemaObserver(client, func(fields []string) {
		fmt.Printf("Decoded source fields: %v\n", fields)
	})

	resolved, err := adapter.Resolve(ctx, lookup)
	if err != nil {
		return fmt.Errorf("vidcore resolution failed: %w", err)
	}

	duration := time.Since(startTime).Round(time.Millisecond)
	fmt.Printf("Native lookup: %d candidates in %s; %d server issues\n", len(resolved.Candidates), duration, len(resolved.Issues))

	tmpDir, err := os.MkdirTemp("", "family-stream-vidcore-check-")
	if err != nil {
		return fmt.Errorf("failed to create temp directory: %w", err)
	}

	sourcesJSON, err := json.MarshalIndent(resolved, "", "  ")
	if err != nil {
		return err
	}

	if err := os.WriteFile(filepath.Join(tmpDir, "sources.json"), sourcesJSON, 0600); err != nil {
		return err
	}
	fmt.Printf("Private source results saved to: %s\n", tmpDir)

	probePool, err := probe.New(client, probe.DefaultLimits())
	if err != nil {
		return fmt.Errorf("failed to initialize prober: %w", err)
	}

	observations := probePool.ProbeAll(ctx, resolved.Candidates)
	for _, obs := range observations {
		fmt.Printf(
			"Probe %-12s: accessible=%-5t sample_bytes=%-8d throughput_measured=%-5t failure=%s\n",
			obs.Source.Server,
			obs.Observation.Accessible,
			obs.Observation.SampleBytes,
			obs.Observation.DeliveryBPS != nil,
			obs.Observation.Failure,
		)
	}

	chosen := selection.Select(lookup, observations)
	selectionJSON, err := json.MarshalIndent(chosen, "", "  ")
	if err != nil {
		return err
	}

	if err := os.WriteFile(filepath.Join(tmpDir, "selection.json"), selectionJSON, 0600); err != nil {
		return err
	}

	if chosen.Selected == nil {
		return fmt.Errorf("native sources resolved, but no candidate passed the measured selection policy")
	}

	fmt.Printf(
		"Selected server: %s; backups: %d; decoded content identity/playback not yet verified\n",
		chosen.Selected.Candidate.Source.Server,
		len(chosen.Backups),
	)

	if *continuity > 0 {
		diagnoseContinuity(ctx, client, resolved.Candidates, *continuity, *contextCheck)
	}

	return nil
}
