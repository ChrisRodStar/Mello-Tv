package httptransport

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"strings"

	"stv-providers/internal/model"
)

// ResolveRequest isolates deduplication/cache scope supplied by the trusted caller.
type ResolveRequest struct {
	Version    string       `json:"version"`
	ContextKey string       `json:"context_key"`
	Lookup     model.Lookup `json:"lookup"`
}

func (h *Handler) resolve(w http.ResponseWriter, r *http.Request) {
	if h == nil || h.engine == nil {
		problem(w, http.StatusInternalServerError, "handler_unavailable")
		return
	}
	mediaType, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || mediaType != "application/json" {
		problem(w, http.StatusUnsupportedMediaType, "unsupported_media_type")
		return
	}
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, MaxRequestBytes))
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			problem(w, http.StatusRequestEntityTooLarge, "request_too_large")
		} else {
			problem(w, http.StatusBadRequest, "invalid_input")
		}
		return
	}
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	var request ResolveRequest
	if !uniqueJSON(body) || decoder.Decode(&request) != nil || decoder.Decode(new(any)) != io.EOF || !validRequest(request) {
		problem(w, http.StatusBadRequest, "invalid_input")
		return
	}
	request.Lookup.ContextKey = request.ContextKey
	ctx, cancel := context.WithTimeout(r.Context(), ResolveTimeout)
	defer cancel()
	if h.engine.Check(ctx) != nil {
		problem(w, http.StatusServiceUnavailable, "vpn_unavailable")
		return
	}
	selection, resolveErr := h.engine.Resolve(ctx, request.Lookup)
	if ctx.Err() != nil {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			problem(w, http.StatusGatewayTimeout, "timeout")
		} else {
			problem(w, http.StatusRequestTimeout, "cancelled")
		}
		return
	}
	// Check again before releasing even a cached result after a guard transition.
	if h.engine.Check(ctx) != nil {
		problem(w, http.StatusServiceUnavailable, "vpn_unavailable")
		return
	}
	if errors.Is(resolveErr, context.DeadlineExceeded) {
		problem(w, http.StatusGatewayTimeout, "timeout")
		return
	}
	if resolveErr != nil {
		var failure *model.Failure
		if !errors.As(resolveErr, &failure) || failure.Stage != "selection" || selection.Version != model.ContractVersion {
			problem(w, http.StatusServiceUnavailable, "resolution_unavailable")
			return
		}
	}
	// Encode before committing headers; never send a truncated selection. The
	// writer bounds serialized output, including arbitrary metadata. The engine
	// separately bounds candidate count and upstream payload allocation.
	if !responseFits(selection) {
		problem(w, http.StatusBadGateway, "response_too_large")
		return
	}
	var buffer boundedJSON
	if json.NewEncoder(&buffer).Encode(selection) != nil {
		problem(w, http.StatusBadGateway, "response_too_large")
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_, _ = w.Write(buffer.Bytes())
}

type boundedJSON struct {
	bytes.Buffer
}

func (b *boundedJSON) Write(data []byte) (int, error) {
	if len(data) > MaxResponseBytes-b.Len() {
		return 0, errors.New("response bound")
	}
	return b.Buffer.Write(data)
}

func validRequest(request ResolveRequest) bool {
	lookup := request.Lookup
	if request.Version != model.ContractVersion || len(request.ContextKey) < 1 || len(request.ContextKey) > 128 || strings.TrimSpace(request.ContextKey) != request.ContextKey || lookup.Title.Validate() != nil {
		return false
	}
	if len(lookup.Title.IMDbID) > 32 || lookup.StartupHeight < 0 || lookup.StartupHeight > 16384 || lookup.Preferences.MaxHeight < 0 || lookup.Device.MaxHeight < 0 || len(lookup.Device.Formats) == 0 || len(lookup.Device.Formats) > 4 || len(lookup.Device.Codecs) > 32 {
		return false
	}
	for _, format := range lookup.Device.Formats {
		if format != model.HLS && format != model.MP4 && format != model.DASH {
			return false
		}
	}
	for _, values := range [][]string{
		lookup.Device.Codecs,
		lookup.Preferences.AudioLanguages,
		lookup.Preferences.SubtitleLanguages,
	} {
		if len(values) > 32 {
			return false
		}
		for _, value := range values {
			if len(value) == 0 || len(value) > 64 {
				return false
			}
		}
	}
	return true
}
