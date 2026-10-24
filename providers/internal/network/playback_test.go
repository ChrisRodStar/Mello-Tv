package network

import (
	"net/http"
	"net/url"
	"testing"

	"stv-providers/internal/model"
)

func TestPlaybackEnvironmentHeaderRemainsOriginAndPathScoped(t *testing.T) {
	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin:     "https://player.example.com",
				PathPrefix: "/api",
				Headers:    map[string]string{"X-Playback-Environment": "hls"},
			},
		},
	}
	for _, target := range []string{
		"https://player.example.com/api/sources",
		"https://player.example.com/media/file.mp4",
		"https://cdn.example.com/api/sources",
	} {
		targetURL, err := url.Parse(target)
		if err != nil {
			t.Fatal(err)
		}
		headers := make(http.Header)
		if err := applyContext(headers, targetURL, fetchContext); err != nil {
			t.Fatal(err)
		}
		wantHeader := target == "https://player.example.com/api/sources"
		if (headers.Get("X-Playback-Environment") == "hls") != wantHeader {
			t.Fatal("playback environment header escaped its scope")
		}
	}
}

func TestCSRFHeaderRemainsOriginAndPathScoped(t *testing.T) {
	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin:     "https://player.example.com",
				PathPrefix: "/protocol",
				Headers:    map[string]string{"X-Csrf-Token": "synthetic-token"},
			},
		},
	}
	for _, target := range []string{
		"https://player.example.com/protocol/probe",
		"https://player.example.com/protocol-other/probe",
		"https://cdn.example.com/protocol/probe",
	} {
		targetURL, err := url.Parse(target)
		if err != nil {
			t.Fatal(err)
		}
		headers := make(http.Header)
		if err := applyContext(headers, targetURL, fetchContext); err != nil {
			t.Fatal(err)
		}
		if (headers.Get("X-Csrf-Token") != "") != (target == "https://player.example.com/protocol/probe") {
			t.Fatal("CSRF header escaped origin/path scope")
		}
	}
}

func TestApplyContextEdgeCases(t *testing.T) {
	validURL, _ := url.Parse("https://example.com/api/test")
	headers := make(http.Header)

	if applyContext(nil, validURL, model.FetchContext{}) == nil {
		t.Fatal("nil header accepted")
	}
	if applyContext(headers, nil, model.FetchContext{}) == nil {
		t.Fatal("nil URL accepted")
	}

	invalidOriginCtx := model.FetchContext{
		Scopes: []model.FetchScope{
			{Origin: "invalid origin://", PathPrefix: "/"},
		},
	}
	if applyContext(headers, validURL, invalidOriginCtx) == nil {
		t.Fatal("invalid scope origin accepted")
	}

	unsupportedHeaderCtx := model.FetchContext{
		Scopes: []model.FetchScope{
			{Origin: "https://example.com", PathPrefix: "/", Headers: map[string]string{"X-Unsafe-Header": "value"}},
		},
	}
	if applyContext(headers, validURL, unsupportedHeaderCtx) == nil {
		t.Fatal("unsupported header accepted")
	}

	missingCookiePathCtx := model.FetchContext{
		Scopes: []model.FetchScope{
			{Origin: "https://example.com", PathPrefix: "/", Cookies: "foo=bar", CookiePath: ""},
		},
	}
	if applyContext(headers, validURL, missingCookiePathCtx) == nil {
		t.Fatal("cookie with empty path accepted")
	}
}

