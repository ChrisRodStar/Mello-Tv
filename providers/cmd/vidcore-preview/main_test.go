package main

import (
	"context"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

type previewFixtureClient struct {
	master   string
	requests []network.Request
}

func (client *previewFixtureClient) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	client.requests = append(client.requests, request)
	responseURL, _ := url.Parse(request.URL)
	body := ""
	contentType := "application/vnd.apple.mpegurl"
	switch responseURL.Path {
	case "/master.m3u8":
		// Resolve relative resources against the final upstream URL after a redirect.
		responseURL, _ = url.Parse("https://cdn.example.com/path/master.m3u8")
		body = client.master
	case "/path/video.m3u8":
		body = "#EXTM3U\n#EXTINF:6,\nvideo.ts\n"
	case "/path/audio.m3u8":
		body = "#EXTM3U\n#EXTINF:6,\naudio.aac\n"
	case "/path/subs.m3u8":
		body = "#EXTM3U\n#EXTINF:6,\nsubtitle.vtt\n"
	case "/path/subtitle.vtt":
		body = "WEBVTT\n\n00:00.000 --> 00:06.000\nSynthetic fixture caption\n"
		contentType = "text/vtt"
	default:
		return network.Response{URL: responseURL, Status: http.StatusNotFound}, nil
	}
	return network.Response{URL: responseURL, Status: http.StatusOK, Header: http.Header{"Content-Type": []string{contentType}}, Body: []byte(body)}, nil
}

func TestPreviewServesSelectedMasterWithSeparateAudioAndSubtitles(t *testing.T) {
	client := &previewFixtureClient{master: `#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",LANGUAGE="en",DEFAULT=YES,AUTOSELECT=YES,URI="audio.m3u8"
#EXT-X-MEDIA:NAME="English",TYPE=SUBTITLES,GROUP-ID="subs",LANGUAGE="en",DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI="subs.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=854x480
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080,AUDIO="audio",SUBTITLES="subs"
video.m3u8
`}
	server := &previewServer{
		client:    client,
		source:    model.Candidate{URL: "https://origin.example.com/master.m3u8", SelectedVariantURL: "https://cdn.example.com/path/video.m3u8", Format: model.HLS},
		resources: make(map[string]resourceEntry),
		idLookup:  make(map[string]string),
		slots:     make(chan struct{}, 4),
	}
	rootPath, err := server.register(server.source.URL, "playlist")
	if err != nil {
		t.Fatal(err)
	}
	requestResource := func(path string) *httptest.ResponseRecorder {
		t.Helper()
		response := httptest.NewRecorder()
		server.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
		if response.Code != http.StatusOK {
			t.Fatalf("resource status = %d", response.Code)
		}
		return response
	}
	masterResponse := requestResource(rootPath)
	localBase, _ := url.Parse("http://example.com/")
	master, err := probe.ParsePlaylist(masterResponse.Body.Bytes(), localBase)
	if err != nil || len(master.Metadata.Variants) != 1 || len(master.Metadata.Tracks) != 2 {
		t.Fatal("rewritten playable master lost rendition associations")
	}
	if master.Metadata.Variants[0].AudioGroup != "audio" || master.Metadata.Variants[0].SubtitleGroup != "subs" || strings.Contains(masterResponse.Body.String(), "low.m3u8") {
		t.Fatal("selected resolution or associated groups were not preserved")
	}
	videoURL, _ := url.Parse(master.Metadata.Variants[0].URL)
	requestResource(videoURL.Path)
	for _, track := range master.Metadata.Tracks {
		trackURL, _ := url.Parse(track.URL)
		response := requestResource(trackURL.Path)
		if track.Kind == "subtitles" {
			for _, line := range strings.Split(response.Body.String(), "\n") {
				if line == "" || strings.HasPrefix(line, "#") {
					continue
				}
				caption := requestResource(line)
				if caption.Header().Get("Content-Type") != "text/vtt" || !strings.HasPrefix(caption.Body.String(), "WEBVTT") {
					t.Fatal("subtitle segment was delivered as video")
				}
			}
		}
	}
	for _, request := range client.requests {
		if strings.HasSuffix(request.URL, "low.m3u8") {
			t.Fatal("unselected rendition was fetched")
		}
	}
}
