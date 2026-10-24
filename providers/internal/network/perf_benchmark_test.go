package network

import (
	"net/http"
	"net/netip"
	"net/url"
	"testing"

	"stv-providers/internal/model"
)

func BenchmarkPublicIP(b *testing.B) {
	ips := []netip.Addr{
		netip.MustParseAddr("93.184.216.34"),
		netip.MustParseAddr("10.0.0.1"),
		netip.MustParseAddr("1.1.1.1"),
		netip.MustParseAddr("192.168.1.1"),
		netip.MustParseAddr("2606:4700:4700::1111"),
		netip.MustParseAddr("127.0.0.1"),
		netip.MustParseAddr("2001:db8::1"),
		netip.MustParseAddr("172.16.0.1"),
	}
	b.ReportAllocs()
	for b.Loop() {
		for _, ip := range ips {
			_ = PublicIP(ip)
		}
	}
}

func BenchmarkValidateURL(b *testing.B) {
	rawURLs := []string{
		"https://media.example.com/stream/master.m3u8?token=xyz123&exp=1700000000",
		"https://cdn.provider.org:443/hls/1080p/index.m3u8",
		"http://edge-server.net/video/segment.ts",
		"https://127.0.0.1/blocked",
		"http://localhost:8080/denied",
	}
	parsed := make([]*url.URL, len(rawURLs))
	for i, raw := range rawURLs {
		parsed[i], _ = url.Parse(raw)
	}
	b.ReportAllocs()
	for b.Loop() {
		for _, u := range parsed {
			_ = ValidateURL(u)
		}
	}
}

func BenchmarkApplyContext(b *testing.B) {
	targetURL, _ := url.Parse("https://media.example.com/api/sources")
	fetchContext := model.FetchContext{
		PublicResources: &model.PublicResourceContext{
			Origin:  "https://player.example.com",
			Referer: "https://player.example.com/",
		},
		Scopes: []model.FetchScope{
			{
				Origin:     "https://media.example.com",
				PathPrefix: "/api",
				Headers: map[string]string{
					"Authorization":          "synthetic-token",
					"X-Playback-Environment": "hls",
				},
				Cookies:    "session=active",
				CookiePath: "/api",
			},
		},
	}
	b.ReportAllocs()
	for b.Loop() {
		header := make(http.Header)
		_ = applyContext(header, targetURL, fetchContext)
	}
}
