package network

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"testing"
	"time"

	"stv-providers/internal/model"
)

func TestDestinationPolicy(t *testing.T) {
	unsafeURLs := []string{
		"http://localhost/media",
		"https://127.0.0.1/media",
		"http://[::ffff:192.168.1.1]/",
		"https://example.com:8443/",
		"https://example.com:8080/",
		"https://user:secret@example.com/",
		"http://0127.0.0.1/media",
		"http://0x7f.0.0.1/",
		"http://2130706433/",
		"http://./",
		"http://example .com/",
		"javascript:alert(1)",
		"ftp://example.com/",
	}
	for _, raw := range unsafeURLs {
		u, e := url.Parse(raw)
		if e == nil && ValidateURL(u) == nil {
			t.Fatalf("unsafe destination accepted: %s", raw)
		}
	}

	if ValidateURL(nil) == nil {
		t.Fatal("nil URL accepted")
	}

	if Origin(nil) != "" {
		t.Fatal("nil URL origin should be empty")
	}

	nonPublicIPs := []string{
		"0.0.0.0",
		"10.0.0.1",
		"100.64.0.1",
		"127.0.0.1",
		"169.254.0.1",
		"172.16.0.1",
		"192.168.1.1",
		"198.18.0.1",
		"203.0.113.1",
		"224.0.0.1",
		"240.0.0.1",
		"255.255.255.255",
		"::1",
		"::ffff:127.0.0.1",
		"2001:db8::1",
		"64:ff9b::a00:1",
		"fc00::1",
		"fe80::1",
		"ff02::1",
	}
	for _, raw := range nonPublicIPs {
		if PublicIP(netip.MustParseAddr(raw)) {
			t.Fatalf("nonpublic address accepted: %s", raw)
		}
	}

	if PublicIP(netip.Addr{}) {
		t.Fatal("zero IP accepted")
	}

	validPublicIPs := []string{
		"93.184.216.34",
		"1.1.1.1",
		"2606:4700:4700::1111",
	}
	for _, raw := range validPublicIPs {
		if !PublicIP(netip.MustParseAddr(raw)) {
			t.Fatalf("valid public address rejected: %s", raw)
		}
	}
}

func TestGuardBlocksBeforeTraffic(t *testing.T) {
	if _, e := New(Options{}); e == nil {
		t.Fatal("missing guard accepted")
	}
	c, e := New(Options{Guard: GuardFunc(func(context.Context) error { return errors.New("not ready") })})
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	if _, e := c.Fetch(context.Background(), Request{URL: "https://example.com/"}); e == nil {
		t.Fatal("guard denial ignored")
	}
}

type fakeTransport func(*http.Request) (*http.Response, error)

func (f fakeTransport) RoundTrip(req *http.Request) (*http.Response, error) { return f(req) }

func TestCrossOriginRedirectDoesNotCarryCredentials(t *testing.T) {
	// Entire transport is synthetic: no DNS or socket request is made.
	c := &Client{guard: GuardFunc(func(context.Context) error { return nil }), timeout: time.Second, maxBytes: 1024}
	requests := 0
	c.http = &http.Client{Transport: fakeTransport(func(req *http.Request) (*http.Response, error) {
		requests++
		if requests == 1 {
			if req.Header.Get("Authorization") != "synthetic" || req.Header.Get("Cookie") != "fixture=value" || req.Header.Get("X-Csrf-Token") != "synthetic-csrf" {
				t.Fatal("source context missing")
			}
			return &http.Response{StatusCode: 302, Header: http.Header{"Location": []string{"https://cdn.example.org/media"}}, Body: io.NopCloser(strings.NewReader(""))}, nil
		}
		if req.Header.Get("Authorization") != "" || req.Header.Get("Cookie") != "" || req.Header.Get("X-Csrf-Token") != "" {
			t.Fatal("credentials crossed origin")
		}
		return &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(strings.NewReader("synthetic media"))}, nil
	}), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	_, err := c.Fetch(context.Background(), Request{URL: "https://example.com/media", Context: model.FetchContext{Scopes: []model.FetchScope{{Origin: "https://example.com", Headers: map[string]string{"Authorization": "synthetic", "X-Csrf-Token": "synthetic-csrf"}, Cookies: "fixture=value", CookiePath: "/"}}}})
	if err != nil || requests != 2 {
		t.Fatalf("synthetic redirect failed: %v", err)
	}
}

func TestPublicResourceIdentityCrossesOriginsWithoutCredentials(t *testing.T) {
	ctx := model.FetchContext{
		PublicResources: &model.PublicResourceContext{Origin: "https://player.example.com", Referer: "https://player.example.com/"},
		Scopes:          []model.FetchScope{{Origin: "https://media.example.com", Headers: map[string]string{"Authorization": "synthetic", "X-Csrf-Token": "synthetic-csrf"}, Cookies: "fixture=value", CookiePath: "/"}},
	}
	cdnURL, _ := url.Parse("https://cdn.example.org/segment.ts")
	header := make(http.Header)
	if err := applyContext(header, cdnURL, ctx); err != nil {
		t.Fatal(err)
	}
	if header.Get("Origin") != "https://player.example.com" || header.Get("Referer") != "https://player.example.com/" {
		t.Fatal("public resource identity lost on CDN")
	}
	if header.Get("Cookie") != "" || header.Get("Authorization") != "" || header.Get("X-Csrf-Token") != "" {
		t.Fatal("source credentials escaped their origin scope")
	}
}

func TestPublicResourceIdentityRejectsCredentialBearingValues(t *testing.T) {
	targetURL, _ := url.Parse("https://cdn.example.org/segment.ts")
	for _, identity := range []string{"https://player.example.com/?token=synthetic", "https://player.example.com/private/session", "https://user:secret@player.example.com/", "http://127.0.0.1/", "https://player.example.com/#secret"} {
		ctx := model.FetchContext{PublicResources: &model.PublicResourceContext{Referer: identity}}
		if err := applyContext(make(http.Header), targetURL, ctx); err == nil {
			t.Fatal("non-public root identity accepted")
		}
	}
}
