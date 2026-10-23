//
//  handler_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit tests for API endpoints, bearer token authorization middleware,
//  health checks, and artwork HTTP redirects.
//

package api_test

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"stv-catalog/internal/api"
	"stv-catalog/internal/db"
	"stv-catalog/internal/service"
	"stv-catalog/internal/upstream"
)

func newTestHandler(t *testing.T, token string) http.Handler {
	t.Helper()
	dir := t.TempDir()
	store, err := db.Open(filepath.Join(dir, "api_test.sqlite"))
	if err != nil {
		t.Fatalf("failed to open test store: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })

	up := upstream.NewUpstream("")
	svc := service.NewService(store, up)
	svc.Now = func() time.Time { return time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC) }

	return api.NewHandler(svc, token)
}

func TestHealthLive(t *testing.T) {
	h := newTestHandler(t, "")
	req := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	rec := httptest.NewRecorder()

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got: %d", rec.Code)
	}

	var body map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("malformed response json: %v", err)
	}

	if body["status"] != "live" || body["version"] != "1" {
		t.Errorf("unexpected live health body: %+v", body)
	}
}

func TestAuthorizationMiddleware(t *testing.T) {
	const secret = "01234567890123456789012345678901"
	h := newTestHandler(t, secret)

	// Unauthorized request
	req1 := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	rec1 := httptest.NewRecorder()
	h.ServeHTTP(rec1, req1)
	if rec1.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401 Unauthorized, got: %d", rec1.Code)
	}

	// Authorized request
	req2 := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	req2.Header.Set("Authorization", "Bearer "+secret)
	rec2 := httptest.NewRecorder()
	h.ServeHTTP(rec2, req2)
	if rec2.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got: %d", rec2.Code)
	}
}

func TestArtworkRedirect(t *testing.T) {
	h := newTestHandler(t, "")
	req := httptest.NewRequest(http.MethodGet, "/internal/v1/artwork/w500/test_poster.jpg", nil)
	rec := httptest.NewRecorder()

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusFound {
		t.Fatalf("expected 302 Found redirect, got: %d", rec.Code)
	}

	expectedLocation := "https://image.tmdb.org/t/p/w500/test_poster.jpg"
	if loc := rec.Header().Get("Location"); loc != expectedLocation {
		t.Errorf("Location header = %q, want %q", loc, expectedLocation)
	}
}

func TestUnknownRoute(t *testing.T) {
	h := newTestHandler(t, "")
	req := httptest.NewRequest(http.MethodGet, "/unknown/endpoint", nil)
	rec := httptest.NewRecorder()

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 Not Found, got: %d", rec.Code)
	}
}

func TestServerTimingHeader(t *testing.T) {
	h := newTestHandler(t, "")
	req := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	rec := httptest.NewRecorder()

	h.ServeHTTP(rec, req)

	timing := rec.Header().Get("Server-Timing")
	if timing == "" || !strings.Contains(timing, "total;dur=") {
		t.Fatalf("expected Server-Timing header with total;dur=, got %q", timing)
	}
}
