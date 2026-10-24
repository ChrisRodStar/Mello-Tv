package network

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/cookiejar"
	"strings"
	"testing"

	"stv-providers/internal/model"
)

type manifestTransport struct {
	calls  int
	header http.Header
}

func (transport *manifestTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	transport.calls++
	return &http.Response{
		StatusCode: http.StatusOK,
		Header:     transport.header.Clone(),
		Body:       io.NopCloser(strings.NewReader("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvideo.m3u8\n")),
		Request:    request,
	}, nil
}

func TestManifestReuseIsScopedIsolatedAndGuarded(t *testing.T) {
	blocked := false
	client, err := New(Options{Guard: GuardFunc(func(context.Context) error {
		if blocked {
			return errors.New("VPN down")
		}
		return nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	transport := &manifestTransport{}
	client.http.Transport = transport
	ctx := WithManifestCache(context.Background())
	request := Request{URL: "https://media.example/master.m3u8", Limit: 1024}
	first, err := client.Fetch(ctx, request)
	if err != nil {
		t.Fatal(err)
	}
	first.Body[0] = 'x'
	first.URL.Path = "/changed"
	second, err := client.Fetch(ctx, request)
	if err != nil || transport.calls != 1 || second.Body[0] != '#' || second.URL.Path != "/master.m3u8" {
		t.Fatal("manifest not reused or caller mutated cache")
	}
	blocked = true
	if _, err := client.Fetch(ctx, request); err == nil {
		t.Fatal("cache bypassed current VPN guard")
	}
	blocked = false
	request.Context = model.FetchContext{PublicResources: &model.PublicResourceContext{Origin: "https://provider.example"}}
	if _, err := client.Fetch(ctx, request); err != nil || transport.calls != 2 {
		t.Fatal("credential contexts shared cache")
	}
	request.Limit = 1
	if _, err := client.Fetch(ctx, request); err == nil {
		t.Fatal("cache bypassed smaller response limit")
	}
	request.Limit = 1024
	if _, err := client.Fetch(WithManifestCache(context.Background()), request); err != nil || transport.calls != 4 {
		t.Fatal("manifest escaped resolution round")
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := client.Fetch(cancelled, request); !errors.Is(err, context.Canceled) {
		t.Fatal("cache ignored cancellation")
	}
}

func TestSessionAndUncacheableManifestsStayFresh(t *testing.T) {
	for _, mode := range []string{"cookie", "no-store", "jar", "sample"} {
		t.Run(mode, func(t *testing.T) {
			client, err := New(Options{Guard: GuardFunc(func(context.Context) error { return nil })})
			if err != nil {
				t.Fatal(err)
			}
			defer client.Close()
			transport := &manifestTransport{header: make(http.Header)}
			client.http.Transport = transport
			request := Request{URL: "https://media.example/master.m3u8"}
			switch mode {
			case "cookie":
				transport.header.Set("Set-Cookie", "session=fixture")
			case "no-store":
				transport.header.Set("Cache-Control", "no-store")
			case "jar":
				request.Jar, _ = cookiejar.New(nil)
			case "sample":
				request.Sample = true
			}
			ctx := WithManifestCache(context.Background())
			for range 2 {
				if _, err := client.Fetch(ctx, request); err != nil {
					t.Fatal(err)
				}
			}
			if transport.calls != 2 {
				t.Fatal("session or noncacheable response reused")
			}
		})
	}
}
