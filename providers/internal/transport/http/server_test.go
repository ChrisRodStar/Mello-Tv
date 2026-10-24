package httptransport

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"stv-providers/internal/model"
)

const testBearer = "synthetic-service-01234567890123456789"

type testEngine struct {
	check   func(context.Context) error
	resolve func(context.Context, model.Lookup) (model.Selection, error)
}

func (e *testEngine) Check(ctx context.Context) error {
	if e.check != nil {
		return e.check(ctx)
	}
	return nil
}

func (e *testEngine) Resolve(ctx context.Context, lookup model.Lookup) (model.Selection, error) {
	if e.resolve != nil {
		return e.resolve(ctx, lookup)
	}
	return model.Selection{Version: model.ContractVersion}, nil
}

func requestBody() []byte {
	body, _ := json.Marshal(ResolveRequest{
		Version:    model.ContractVersion,
		ContextKey: "synthetic-household",
		Lookup: model.Lookup{
			Title: model.Title{
				Type:   model.Movie,
				TMDBID: 550,
			},
			Device: model.Device{Formats: []model.Format{model.HLS}},
		},
	})
	return body
}

func invoke(handler http.Handler, method, path string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+testBearer)
	request.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	return recorder
}

func TestNew_Validation(t *testing.T) {
	engine := &testEngine{}
	if _, err := New(nil, []byte(testBearer)); err == nil {
		t.Error("expected error for nil engine")
	}
	if _, err := New(engine, []byte("too-short")); err == nil {
		t.Error("expected error for short bearer")
	}
	if _, err := New(engine, []byte(strings.Repeat("a", 4097))); err == nil {
		t.Error("expected error for long bearer")
	}
	nonASCII := append([]byte(testBearer[:31]), 0x1f)
	if _, err := New(engine, nonASCII); err == nil {
		t.Error("expected error for non-ASCII bearer")
	}
}

func TestNilGuards(t *testing.T) {
	var nilHandler *Handler
	nilHandler.Drain() // Must not panic

	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/health/livez", nil)
	nilHandler.ServeHTTP(rec, req)
	if rec.Code != http.StatusInternalServerError {
		t.Errorf("expected 500 for nil handler, got %d", rec.Code)
	}

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()

	ctx := context.Background()
	handler, _ := New(&testEngine{}, []byte(testBearer))

	var nilCtx context.Context
	if err := Serve(nilCtx, listener, handler); err == nil || err.Error() != "context cannot be nil" {
		t.Errorf("expected 'context cannot be nil', got %v", err)
	}
	if err := Serve(ctx, nil, handler); err == nil || err.Error() != "listener and handler required" {
		t.Errorf("expected 'listener and handler required', got %v", err)
	}
	if err := Serve(ctx, listener, nil); err == nil || err.Error() != "listener and handler required" {
		t.Errorf("expected 'listener and handler required', got %v", err)
	}
}

func TestAuthenticationValidationAndUnavailableGuard(t *testing.T) {
	engine := &testEngine{check: func(context.Context) error {
		return errors.New("secret upstream")
	}}
	handler, err := New(engine, []byte(testBearer))
	if err != nil {
		t.Fatal(err)
	}
	unauthorized := httptest.NewRecorder()
	handler.ServeHTTP(unauthorized, httptest.NewRequest("POST", "/internal/v1/resolve", bytes.NewReader(requestBody())))
	if unauthorized.Code != 401 {
		t.Fatal(unauthorized.Code)
	}
	for _, scenario := range []struct {
		method, path string
		body         []byte
		status       int
	}{
		{
			"GET",
			"/health/livez",
			nil,
			200,
		},
		{
			"GET",
			"/health/readyz",
			nil,
			503,
		},
		{
			"POST",
			"/internal/v1/resolve",
			requestBody(),
			503,
		},
		{
			"POST",
			"/internal/v1/resolve",
			[]byte(`{"unknown":1}`),
			400,
		},
		{
			"POST",
			"/internal/v1/resolve",
			append(requestBody(), []byte(` {}`)...),
			400,
		},
		{
			"POST",
			"/internal/v1/resolve",
			bytes.Repeat([]byte("x"), MaxRequestBytes+1),
			413,
		},
		{
			"POST",
			"/internal/v1/resolve?token=secret",
			requestBody(),
			400,
		},
		{
			"GET",
			"/internal/v1/resolve",
			nil,
			405,
		},
	} {
		response := invoke(handler, scenario.method, scenario.path, scenario.body)
		if response.Code != scenario.status || strings.Contains(response.Body.String(), "secret") {
			t.Fatalf("status %d expected %d", response.Code, scenario.status)
		}
		if response.Header().Get("Cache-Control") != "private, no-store" {
			t.Fatal("missing privacy policy")
		}
	}
}

func TestSelectionScopeAndGuardTransition(t *testing.T) {
	checks := 0
	engine := &testEngine{
		check: func(context.Context) error {
			checks++
			if checks == 4 {
				return errors.New("guard lost")
			}
			return nil
		},
		resolve: func(_ context.Context, lookup model.Lookup) (model.Selection, error) {
			if lookup.ContextKey != "synthetic-household" || lookup.Title.TMDBID != 550 {
				t.Fatal("lookup changed")
			}
			return model.Selection{Version: model.ContractVersion}, model.Fail(model.Unavailable, "selection", "no eligible source")
		},
	}
	handler, _ := New(engine, []byte(testBearer))
	first := invoke(handler, "POST", "/internal/v1/resolve", requestBody())
	if first.Code != 200 {
		t.Fatal(first.Code)
	}
	var result model.Selection
	if json.Unmarshal(first.Body.Bytes(), &result) != nil || result.Version != model.ContractVersion || result.Selected != nil {
		t.Fatal("no-source envelope changed")
	}
	if invoke(handler, "POST", "/internal/v1/resolve", requestBody()).Code != 503 {
		t.Fatal("result escaped lost guard")
	}
}

func TestOverloadDiagnosticsAndDrain(t *testing.T) {
	handler, _ := New(&testEngine{}, []byte(testBearer))
	for index := 0; index < MaxResolutions; index++ {
		handler.resolutions <- struct{}{}
	}
	if invoke(handler, "POST", "/internal/v1/resolve", requestBody()).Code != 429 {
		t.Fatal("unbounded admission")
	}
	if invoke(handler, "GET", "/health/readyz", nil).Code != 200 {
		t.Fatal("diagnostics blocked")
	}
	handler.Drain()
	if invoke(handler, "POST", "/internal/v1/resolve", requestBody()).Code != 503 {
		t.Fatal("draining admitted work")
	}
	if invoke(handler, "GET", "/health/livez", nil).Code != 200 {
		t.Fatal("drain hid health")
	}
}

func TestResponseLimitAndCancellation(t *testing.T) {
	engine := &testEngine{resolve: func(context.Context, model.Lookup) (model.Selection, error) {
		return model.Selection{
			Version:  model.ContractVersion,
			Rejected: map[string]string{"id": strings.Repeat("x", MaxResponseBytes)},
		}, nil
	}}
	handler, _ := New(engine, []byte(testBearer))
	result := invoke(handler, "POST", "/internal/v1/resolve", requestBody())
	if result.Code != 502 || result.Body.Len() > 128 {
		t.Fatal("oversized response committed")
	}
	engine.resolve = func(ctx context.Context, _ model.Lookup) (model.Selection, error) {
		<-ctx.Done()
		return model.Selection{}, ctx.Err()
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	request := httptest.NewRequest("POST", "/internal/v1/resolve", bytes.NewReader(requestBody())).WithContext(ctx)
	request.Header.Set("Authorization", "Bearer "+testBearer)
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != 408 {
		t.Fatal("cancellation not propagated")
	}
}

func TestAmbiguousAndDeepJSONRejected(t *testing.T) {
	for _, value := range []string{
		`{"version":"providers.v1","version":"other"}`,
		strings.Repeat("[", 40) + "0" + strings.Repeat("]", 40),
	} {
		if uniqueJSON([]byte(value)) {
			t.Fatal("ambiguous or excessive JSON admitted")
		}
	}
	if !uniqueJSON(requestBody()) {
		t.Fatal("valid request rejected")
	}
}

func TestEngineDeadlineReturnsGatewayTimeout(t *testing.T) {
	engine := &testEngine{resolve: func(context.Context, model.Lookup) (model.Selection, error) {
		return model.Selection{}, context.DeadlineExceeded
	}}
	handler, _ := New(engine, []byte(testBearer))
	response := invoke(handler, "POST", "/internal/v1/resolve", requestBody())
	if response.Code != 504 {
		t.Fatal("engine deadline did not return a timeout")
	}
}

func TestCaseAliasesAreRejectedBeforeResolution(t *testing.T) {
	handler, _ := New(&testEngine{}, []byte(testBearer))
	for _, body := range []string{
		strings.Replace(string(requestBody()), `"lookup":{`, `"lookup":{"refresh":false,"Refresh":true,`, 1),
		strings.Replace(string(requestBody()), `"version":`, `"Version":`, 1),
	} {
		if invoke(handler, "POST", "/internal/v1/resolve", []byte(body)).Code != 400 {
			t.Fatal("case alias admitted")
		}
	}
}

func TestListenerFailureAndForcedDrainJoinHandlersBeforeReturn(t *testing.T) {
	for _, failListener := range []bool{
		true,
		false,
	} {
		started, cancelled, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
		var releaseOnce sync.Once
		releaseHandler := func() {
			releaseOnce.Do(func() {
				close(release)
			})
		}
		engine := &testEngine{resolve: func(ctx context.Context, _ model.Lookup) (model.Selection, error) {
			close(started)
			<-ctx.Done()
			close(cancelled)
			<-release
			return model.Selection{}, ctx.Err()
		}}
		handler, _ := New(engine, []byte(testBearer))
		listener, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithCancel(context.Background())
		served := make(chan error, 1)
		go func() {
			served <- serveWithDrain(ctx, listener, handler, 20*time.Millisecond)
		}()
		request, _ := http.NewRequest(http.MethodPost, "http://"+listener.Addr().String()+"/internal/v1/resolve", bytes.NewReader(requestBody()))
		request.Header.Set("Authorization", "Bearer "+testBearer)
		request.Header.Set("Content-Type", "application/json")
		clientDone := make(chan struct{})
		go func() {
			defer close(clientDone)
			response, _ := (&http.Client{Timeout: 2 * time.Second}).Do(request)
			if response != nil {
				response.Body.Close()
			}
		}()
		select {
		case <-started:
		case <-time.After(time.Second):
			releaseHandler()
			cancel()
			listener.Close()
			t.Fatal("handler did not start")
		}
		if failListener {
			listener.Close()
		} else {
			cancel()
		}
		select {
		case <-cancelled:
		case <-time.After(time.Second):
			releaseHandler()
			cancel()
			listener.Close()
			t.Fatal("handler context not cancelled")
		}
		select {
		case <-served:
			releaseHandler()
			t.Fatal("server returned before handler joined")
		case <-time.After(50 * time.Millisecond):
		}
		releaseHandler()
		select {
		case serveErr := <-served:
			if (serveErr != nil) != failListener {
				t.Fatal("unexpected serve result")
			}
		case <-time.After(time.Second):
			t.Fatal("server did not join")
		}
		cancel()
		listener.Close()
		<-clientDone
	}
}

func BenchmarkHandler_HealthLivez(b *testing.B) {
	handler, _ := New(&testEngine{}, []byte(testBearer))
	req := httptest.NewRequest("GET", "/health/livez", nil)
	req.Header.Set("Authorization", "Bearer "+testBearer)
	w := httptest.NewRecorder()

	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		w.Body.Reset()
		handler.ServeHTTP(w, req)
	}
}

func BenchmarkHandler_HealthReadyz(b *testing.B) {
	handler, _ := New(&testEngine{}, []byte(testBearer))
	req := httptest.NewRequest("GET", "/health/readyz", nil)
	req.Header.Set("Authorization", "Bearer "+testBearer)
	w := httptest.NewRecorder()

	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		w.Body.Reset()
		handler.ServeHTTP(w, req)
	}
}

func BenchmarkHandler_Resolve(b *testing.B) {
	engine := &testEngine{
		resolve: func(ctx context.Context, lookup model.Lookup) (model.Selection, error) {
			return model.Selection{
				Version: model.ContractVersion,
				Selected: &model.Ranked{
					Candidate: model.Probed{
						Source: model.Candidate{
							ID:       "bench-stream",
							Provider: model.VidLink,
							Title:    lookup.Title,
							URL:      "https://stream.example.com/live.m3u8",
							Format:   model.HLS,
						},
						Observation: model.Observation{
							Accessible: true,
							Format:     model.HLS,
						},
					},
					Score: 100.0,
				},
			}, nil
		},
	}
	handler, _ := New(engine, []byte(testBearer))
	body := requestBody()
	w := httptest.NewRecorder()

	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		w.Body.Reset()
		req := httptest.NewRequest("POST", "/internal/v1/resolve", bytes.NewReader(body))
		req.Header.Set("Authorization", "Bearer "+testBearer)
		req.Header.Set("Content-Type", "application/json")
		handler.ServeHTTP(w, req)
	}
}

func BenchmarkUniqueJSON(b *testing.B) {
	body := requestBody()
	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		if !uniqueJSON(body) {
			b.Fatal("unexpected false from uniqueJSON")
		}
	}
}

func BenchmarkResponseFits(b *testing.B) {
	selection := model.Selection{
		Version: model.ContractVersion,
		Selected: &model.Ranked{
			Candidate: model.Probed{
				Source: model.Candidate{
					ID:       "bench-stream",
					Provider: model.VidLink,
					URL:      "https://stream.example.com/live.m3u8",
					Format:   model.HLS,
				},
				Observation: model.Observation{
					Accessible: true,
					Format:     model.HLS,
				},
			},
			Score: 100.0,
		},
	}
	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		if !responseFits(selection) {
			b.Fatal("unexpected false from responseFits")
		}
	}
}
