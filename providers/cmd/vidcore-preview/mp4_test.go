package main

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"testing"

	"family-stream-providers/internal/network"
)

type mp4PreviewFixture struct {
	requests    []network.Request
	ignoreRange bool
	changeTotal bool
}

func (fixture *mp4PreviewFixture) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	fixture.requests = append(fixture.requests, request)
	var start, end int64
	if _, err := fmt.Sscanf(request.Headers["Range"], "bytes=%d-%d", &start, &end); err != nil {
		return network.Response{}, err
	}
	total := int64(100 * 1024 * 1024)
	if fixture.changeTotal && len(fixture.requests) > 1 {
		total++
	}
	status := http.StatusPartialContent
	if fixture.ignoreRange {
		status = http.StatusOK
	}
	resourceURL, err := url.Parse(request.URL)
	if err != nil {
		return network.Response{}, err
	}
	return network.Response{
		URL:    resourceURL,
		Status: status,
		Header: http.Header{"Content-Range": []string{fmt.Sprintf("bytes %d-%d/%d", start, end, total)}},
		Body:   make([]byte, end-start+1),
	}, nil
}

func TestPreviewMP4StreamsRequestedExtentInBoundedChunks(t *testing.T) {
	fixture := &mp4PreviewFixture{}
	server := &previewServer{client: fixture}
	request := httptest.NewRequest(http.MethodGet, "/resource/fixture", nil)
	startOffset := int64(12345678)
	requestedLength := mp4PreviewRangeLimit + 256*1024
	request.Header.Set("Range", fmt.Sprintf("bytes=%d-%d", startOffset, startOffset+requestedLength-1))
	response := httptest.NewRecorder()
	server.serveMP4(request.Context(), response, request, resourceEntry{URL: "https://example.com/movie.mp4", Kind: "mp4"})
	if response.Code != http.StatusPartialContent || response.Body.Len() != int(requestedLength) || len(fixture.requests) != 3 {
		t.Fatal("MP4 preview did not honor its range cap")
	}
	start, end, _, err := parseContentRange(response.Header().Get("Content-Range"))
	if err != nil || start != startOffset || end-start+1 != requestedLength || fixture.requests[1].Limit != mp4PreviewRangeLimit || fixture.requests[2].Limit != 256*1024 {
		t.Fatal("seek offset or bounded upstream request changed")
	}
}

type cancellingPreviewWriter struct {
	*httptest.ResponseRecorder
	cancel context.CancelFunc
}

func (writer *cancellingPreviewWriter) Write(body []byte) (int, error) {
	bytesWritten, err := writer.ResponseRecorder.Write(body)
	writer.cancel()
	return bytesWritten, err
}

func TestPreviewMP4CancellationStopsBeforeNextChunk(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	fixture := &mp4PreviewFixture{}
	server := &previewServer{client: fixture}
	request := httptest.NewRequest(http.MethodGet, "/resource/fixture", nil).WithContext(ctx)
	request.Header.Set("Range", "bytes=0-33554431")
	writer := &cancellingPreviewWriter{
		ResponseRecorder: httptest.NewRecorder(),
		cancel:           cancel,
	}
	server.serveMP4(ctx, writer, request, resourceEntry{URL: "https://example.com/movie.mp4", Kind: "mp4"})
	if len(fixture.requests) != 2 || int64(writer.Body.Len()) != mp4PreviewRangeLimit {
		t.Fatal("preview continued fetching after the player cancelled")
	}
}

func TestPreviewMP4HeadAndRejectedUpstream(t *testing.T) {
	fixture := &mp4PreviewFixture{}
	server := &previewServer{client: fixture}
	request := httptest.NewRequest(http.MethodHead, "/resource/fixture", nil)
	response := httptest.NewRecorder()
	server.serveMP4(request.Context(), response, request, resourceEntry{URL: "https://example.com/movie.mp4", Kind: "mp4"})
	if response.Code != http.StatusOK || response.Body.Len() != 0 || response.Header().Get("Content-Length") != strconv.Itoa(100*1024*1024) || len(fixture.requests) != 1 {
		t.Fatal("HEAD did not return bounded resource metadata")
	}
	for _, upstream := range []*mp4PreviewFixture{{ignoreRange: true}, {changeTotal: true}} {
		server.client = upstream
		request = httptest.NewRequest(http.MethodGet, "/resource/fixture", nil)
		request.Header.Set("Range", "bytes=100-199")
		response = httptest.NewRecorder()
		server.serveMP4(request.Context(), response, request, resourceEntry{URL: "https://example.com/movie.mp4", Kind: "mp4"})
		if response.Code != http.StatusBadGateway {
			t.Fatal("ignored range or unstable resource accepted")
		}
	}
}

func TestPreviewRangeRejectsMalformedOverflowAndMultipleRanges(t *testing.T) {
	for _, value := range []string{"bytes=-", "bytes=5-4", "bytes=100-", "bytes=0-1,2-3", "bytes=999999999999999999999999999-", "bytes=-0"} {
		if _, _, err := previewRange(value, 100); err == nil {
			t.Fatal("invalid range accepted")
		}
	}
	start, end, err := previewRange("bytes=-20", 100)
	if err != nil || start != 80 || end != 99 {
		t.Fatal("suffix range changed")
	}
}
