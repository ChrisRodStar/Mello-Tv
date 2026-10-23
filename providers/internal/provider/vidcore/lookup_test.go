package vidcore

import (
	"family-stream-providers/internal/model"
	"testing"
)

func TestNormalizeObservedWebVTTTracks(t *testing.T) {
	payload := []byte(`{"url":"https://media.example.com/movie.m3u8","tracks":[{"file":"https://subtitles.example.com/en.vtt","label":"English"},{"file":"https://subtitles.example.com/other.vtt","label":"Unmapped language"},{"file":"http://127.0.0.1/private.vtt","label":"English"},{"file":"https://subtitles.example.com/unknown.srt","label":"English"}]}`)
	candidate, err := normalizeSource(payload, model.Title{Type: model.Movie, TMDBID: 27205}, "Supreme")
	if err != nil {
		t.Fatal(err)
	}
	tracks := candidate.Metadata.Tracks
	if len(tracks) != 2 || tracks[0].Kind != "subtitles" || tracks[0].Language != "en" || tracks[1].Language != "" {
		t.Fatalf("observed subtitle normalization incorrect: %+v", tracks)
	}
	if candidate.Metadata.TracksKnown || candidate.Metadata.ProviderData["tracks"] == nil {
		t.Fatal("normalization guessed complete track availability or dropped raw metadata")
	}
	if len(candidate.Context.Scopes) != 1 || candidate.Context.Scopes[0].Origin != "https://media.example.com:443" {
		t.Fatal("subtitle normalization broadened media credential scope")
	}
}

func TestPublicResourceContextHonorsNoReferrer(t *testing.T) {
	candidate, err := normalizeSource([]byte(`{"url":"https://media.example.com/movie.m3u8","noReferrer":true}`), model.Title{Type: model.Movie, TMDBID: 27205}, "Supreme")
	if err != nil {
		t.Fatal(err)
	}
	if candidate.Context.PublicResources == nil || candidate.Context.PublicResources.Origin != origin || candidate.Context.PublicResources.Referer != "" {
		t.Fatal("public resource context ignored noReferrer")
	}
}
