// Regenerated in an isolated module containing unmodified actual provider models.
package main

import (
	"encoding/json"
	"family-stream-providers/internal/model"
	"os"
	"path/filepath"
	"time"
)

func ptr[T any](v T) *T { return &v }
func main() {
	now := time.Date(2030, 1, 1, 0, 0, 0, 0, time.UTC)
	base := model.Ranked{Candidate: model.Probed{Source: model.Candidate{
		ID: "synthetic-source-1", Provider: model.VidCore,
		Title: model.Title{Type: model.Movie, TMDBID: 27205, IMDbID: "tt1375666"},
		URL:   "https://media.example.com/master.m3u8", Format: model.HLS,
		Metadata: model.Metadata{TracksKnown: false, ProviderData: map[string]json.RawMessage{"unknown": json.RawMessage(`{"preserve":true,"big":18446744073709551617,"decimal":0.12345678901234567890123456789}`)}},
	}, Observation: model.Observation{At: now, Accessible: true, Format: model.HLS, FirstByte: ptr(100 * time.Millisecond), DeliveryBPS: ptr(float64(24000000)), SamplesCompleted: 3, SampleBytes: 393216, ProtectionKnown: true}}, Score: 40}
	cases := map[string]model.Selection{}
	selection := func(r model.Ranked) model.Selection {
		return model.Selection{Version: model.ContractVersion, Selected: &r, Providers: []model.ProviderStatus{{Provider: r.Candidate.Source.Provider, Duration: time.Second, CandidateCount: 1}}}
	}
	cases["movie-null"] = selection(base)
	tv := base
	tv.Candidate.Source.Title = model.Title{Type: model.TV, TMDBID: 60625, Season: ptr(0), Episode: ptr(1), MyAnimeListID: ptr(int64(1))}
	cases["tv-special"] = selection(tv)
	rendition := base
	variant := model.Variant{URL: "https://media.example.com/video/1080.m3u8", Width: ptr(1920), Height: ptr(1080), Bitrate: ptr(int64(6000000)), Codecs: []string{"avc1.640028", "mp4a.40.2"}, AudioGroup: "audio-en", SubtitleGroup: "subs-en"}
	rendition.Candidate.Source.SelectedVariantURL = variant.URL
	rendition.Candidate.Observation.Variant = &variant
	rendition.Candidate.Observation.Metadata = model.Metadata{TracksKnown: true, Variants: []model.Variant{variant}, Tracks: []model.Track{{Kind: "audio", Language: "en", Group: "audio-en", URL: "https://media.example.com/audio/en.m3u8", Default: true}, {Kind: "subtitles", Language: "en", Group: "subs-en", URL: "https://media.example.com/subs/en.m3u8"}}}
	cases["renditions"] = selection(rendition)
	encrypted := base
	encrypted.Candidate.Observation.Encrypted = true
	cases["encrypted-hls"] = selection(encrypted)
	mp4 := base
	mp4.Candidate.Source.Format = model.MP4
	mp4.Candidate.Source.URL = "https://media.example.com/movie.mp4"
	mp4.Candidate.Observation.Format = model.MP4
	mp4.Candidate.Observation.FirstByte = nil
	mp4.Candidate.Observation.DeliveryBPS = nil
	cases["progressive-mp4"] = selection(mp4)
	backup := mp4
	backup.Candidate.Source.ID = "synthetic-backup-1"
	backup.Candidate.Source.Provider = model.VidLink
	backup2 := base
	backup2.Candidate.Source.ID = "synthetic-backup-2"
	backup2.Candidate.Source.Provider = model.VidSrc
	ordered := selection(rendition)
	ordered.Backups = []model.Ranked{backup, backup2}
	ordered.Rejected = map[string]string{}
	cases["ordered-backups"] = ordered
	cases["no-selection"] = model.Selection{Version: model.ContractVersion}
	for name, value := range cases {
		data, err := json.MarshalIndent(value, "", "  ")
		if err != nil {
			panic(err)
		}
		if err = os.WriteFile(filepath.Join(os.Args[1], name+".json"), append(data, '\n'), 0644); err != nil {
			panic(err)
		}
	}
}
