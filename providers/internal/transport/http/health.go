package httptransport

import (
	"context"
	"net/http"
	"time"
)

type readyEngine interface {
	Ready(context.Context) error
}

var (
	livezPayload  = []byte("{\"live\":true}\n")
	readyzPayload = []byte("{\"ready\":true,\"version\":\"providers.v1\"}\n")
)

func (h *Handler) health(w http.ResponseWriter, r *http.Request) {
	if h == nil || h.engine == nil {
		problem(w, http.StatusInternalServerError, "handler_unavailable")
		return
	}
	if r.URL.Path == "/health/livez" {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(livezPayload)
		return
	}
	select {
	case h.diagnostics <- struct{}{}:
		defer func() {
			<-h.diagnostics
		}()
	default:
		problem(w, http.StatusTooManyRequests, "overloaded")
		return
	}
	if h.draining.Load() {
		problem(w, http.StatusServiceUnavailable, "draining")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 2*time.Second)
	defer cancel()
	check := h.engine.Check
	if admission, ok := h.engine.(readyEngine); ok {
		check = admission.Ready
	}
	if check(ctx) != nil {
		problem(w, http.StatusServiceUnavailable, "vpn_unavailable")
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_, _ = w.Write(readyzPayload)
}
