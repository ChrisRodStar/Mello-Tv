package main

import (
	"context"
	"fmt"
	"net/http"
	"regexp"
	"strconv"
	"strings"

	"family-stream-providers/internal/network"
)

const mp4PreviewRangeLimit = int64(8 * 1024 * 1024)

var contentRangePattern = regexp.MustCompile(`^bytes ([0-9]+)-([0-9]+)/([0-9]+)$`)

func parseContentRange(value string) (int64, int64, int64, error) {
	parts := contentRangePattern.FindStringSubmatch(value)
	if len(parts) != 4 {
		return 0, 0, 0, fmt.Errorf("invalid partial response")
	}
	start, startErr := strconv.ParseInt(parts[1], 10, 64)
	end, endErr := strconv.ParseInt(parts[2], 10, 64)
	total, totalErr := strconv.ParseInt(parts[3], 10, 64)
	if startErr != nil || endErr != nil || totalErr != nil || total <= 0 || start > end || end >= total {
		return 0, 0, 0, fmt.Errorf("partial response bounds rejected")
	}
	return start, end, total, nil
}

func previewRange(value string, total int64) (int64, int64, error) {
	if value == "" {
		return 0, total - 1, nil
	}
	if !byteRangeHeader.MatchString(value) || value == "bytes=-" {
		return 0, 0, fmt.Errorf("invalid range")
	}
	startText, endText, _ := strings.Cut(strings.TrimPrefix(value, "bytes="), "-")
	if startText == "" {
		length, err := strconv.ParseInt(endText, 10, 64)
		if err != nil || length <= 0 {
			return 0, 0, fmt.Errorf("invalid suffix range")
		}
		length = min(length, total)
		return total - length, total - 1, nil
	}
	start, err := strconv.ParseInt(startText, 10, 64)
	if err != nil || start >= total {
		return 0, 0, fmt.Errorf("range start outside resource")
	}
	end := total - 1
	if endText != "" {
		end, err = strconv.ParseInt(endText, 10, 64)
		if err != nil || end < start {
			return 0, 0, fmt.Errorf("invalid range end")
		}
		end = min(end, total-1)
	}
	return start, end, nil
}

// serveMP4 is a bounded local diagnostic proxy, not the production Rust relay.
// Even metadata/range discovery uses the controlled client and source context.
func (p *previewServer) serveMP4(ctx context.Context, w http.ResponseWriter, r *http.Request, entry resourceEntry) {
	if requestedRange := r.Header.Get("Range"); len(requestedRange) < 100 && (requestedRange == "" || byteRangeHeader.MatchString(requestedRange)) {
		fmt.Printf("Preview MP4 method=%s range=%s\n", r.Method, requestedRange)
	}
	discovery, err := p.client.Fetch(ctx, network.Request{
		URL:     entry.URL,
		Context: p.source.Context,
		Headers: map[string]string{"Range": "bytes=0-1"},
		Limit:   2,
	})
	if err != nil || discovery.Status != http.StatusPartialContent {
		http.Error(w, "MP4 range discovery rejected", http.StatusBadGateway)
		return
	}
	initialStart, initialEnd, total, err := parseContentRange(discovery.Header.Get("Content-Range"))
	if err != nil || initialStart != 0 || initialEnd != 1 || len(discovery.Body) != 2 {
		http.Error(w, "invalid MP4 range discovery", http.StatusBadGateway)
		return
	}
	if r.Method == http.MethodHead {
		w.Header().Set("Content-Type", "video/mp4")
		w.Header().Set("Content-Length", strconv.FormatInt(total, 10))
		w.Header().Set("Accept-Ranges", "bytes")
		w.Header().Set("Cache-Control", "private, no-store")
		return
	}
	start, end, err := previewRange(r.Header.Get("Range"), total)
	if err != nil {
		w.Header().Set("Content-Range", "bytes */"+strconv.FormatInt(total, 10))
		http.Error(w, "MP4 byte range rejected", http.StatusRequestedRangeNotSatisfiable)
		return
	}
	// AVFoundation requires the advertised range to match its requested extent.
	// Keep that extent while reading only one bounded upstream chunk at a time.
	if deadline, present := ctx.Deadline(); present {
		_ = http.NewResponseController(w).SetWriteDeadline(deadline)
	}
	for position := start; position <= end; {
		if ctx.Err() != nil {
			return
		}
		length := min(end-position+1, mp4PreviewRangeLimit)
		chunkEnd := position + length - 1
		response := discovery
		if position != 0 || chunkEnd != 1 {
			response, err = p.client.Fetch(ctx, network.Request{
				URL:     entry.URL,
				Context: p.source.Context,
				Headers: map[string]string{"Range": fmt.Sprintf("bytes=%d-%d", position, chunkEnd)},
				Limit:   length,
			})
		}
		actualStart, actualEnd, actualTotal, rangeErr := parseContentRange(response.Header.Get("Content-Range"))
		if err != nil || response.Status != http.StatusPartialContent || rangeErr != nil ||
			actualStart != position || actualEnd != chunkEnd || actualTotal != total || int64(len(response.Body)) != length {
			if position == start {
				http.Error(w, "MP4 media range rejected or changed", http.StatusBadGateway)
			}
			return
		}
		if position == start {
			w.Header().Set("Content-Type", "video/mp4")
			w.Header().Set("Accept-Ranges", "bytes")
			w.Header().Set("Content-Length", strconv.FormatInt(end-start+1, 10))
			w.Header().Set("Cache-Control", "private, no-store")
			if r.Header.Get("Range") != "" {
				w.Header().Set("Content-Range", fmt.Sprintf("bytes %d-%d/%d", start, end, total))
				w.WriteHeader(http.StatusPartialContent)
			}
		}
		if _, err := w.Write(response.Body); err != nil {
			return
		}
		position = chunkEnd + 1
	}
}
