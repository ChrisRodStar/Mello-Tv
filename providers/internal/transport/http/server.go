// Package httptransport exposes the private providers.v1 service boundary.
package httptransport

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"stv-providers/internal/model"
)

const (
	MaxRequestBytes  = 32 * 1024
	MaxResponseBytes = 2 * 1024 * 1024
	MaxResolutions   = 8
	MaxConnections   = 64
	ResolveTimeout   = 30 * time.Second
)

// Engine is consumed by this transport; production uses config.Service.
type Engine interface {
	Check(context.Context) error
	Resolve(context.Context, model.Lookup) (model.Selection, error)
}

type Handler struct {
	engine      Engine
	bearer      [32]byte
	// The capacity is the admission limit, not a queue. Admission never waits.
	resolutions chan struct{}
	diagnostics chan struct{}
	draining    atomic.Bool
}

func New(engine Engine, bearer []byte) (*Handler, error) {
	if engine == nil || len(bearer) < 32 || len(bearer) > 4096 {
		return nil, errors.New("engine and 32..4096 byte service bearer required")
	}
	for _, value := range bearer {
		if value < 33 || value > 126 {
			return nil, errors.New("ASCII bearer token required")
		}
	}
	return &Handler{
		engine:      engine,
		bearer:      sha256.Sum256(bearer),
		resolutions: make(chan struct{}, MaxResolutions),
		diagnostics: make(chan struct{}, 1),
	}, nil
}

func (h *Handler) Drain() {
	if h == nil {
		return
	}
	h.draining.Store(true)
}

var problemPayloads = map[string][]byte{
	"unauthorized":           []byte("{\"code\":\"unauthorized\"}\n"),
	"invalid_input":          []byte("{\"code\":\"invalid_input\"}\n"),
	"not_found":              []byte("{\"code\":\"not_found\"}\n"),
	"method_not_allowed":     []byte("{\"code\":\"method_not_allowed\"}\n"),
	"overloaded":             []byte("{\"code\":\"overloaded\"}\n"),
	"draining":               []byte("{\"code\":\"draining\"}\n"),
	"vpn_unavailable":        []byte("{\"code\":\"vpn_unavailable\"}\n"),
	"timeout":                []byte("{\"code\":\"timeout\"}\n"),
	"cancelled":              []byte("{\"code\":\"cancelled\"}\n"),
	"resolution_unavailable": []byte("{\"code\":\"resolution_unavailable\"}\n"),
	"response_too_large":     []byte("{\"code\":\"response_too_large\"}\n"),
	"unsupported_media_type": []byte("{\"code\":\"unsupported_media_type\"}\n"),
	"request_too_large":      []byte("{\"code\":\"request_too_large\"}\n"),
}

func problem(w http.ResponseWriter, status int, code string) {
	w.Header().Set("Content-Type", "application/problem+json")
	w.WriteHeader(status)
	if payload, ok := problemPayloads[code]; ok {
		_, _ = w.Write(payload)
		return
	}
	// Fall back to encoder for any non-standard code.
	_ = json.NewEncoder(w).Encode(struct {
		Code string `json:"code"`
	}{Code: code})
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if h == nil {
		problem(w, http.StatusInternalServerError, "handler_unavailable")
		return
	}
	w.Header().Set("Cache-Control", "private, no-store")
	w.Header().Set("Referrer-Policy", "no-referrer")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	authorization := r.Header.Values("Authorization")
	if len(authorization) != 1 {
		problem(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	token, ok := strings.CutPrefix(authorization[0], "Bearer ")
	if !ok || len(token) == 0 {
		problem(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	digest := sha256.Sum256([]byte(token))
	if subtle.ConstantTimeCompare(digest[:], h.bearer[:]) != 1 {
		problem(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	if r.URL.RawQuery != "" || r.URL.ForceQuery {
		problem(w, http.StatusBadRequest, "invalid_input")
		return
	}
	switch r.URL.Path {
	case "/health/livez", "/health/readyz":
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", "GET")
			problem(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		h.health(w, r)
	case "/internal/v1/resolve":
		if r.Method != http.MethodPost {
			w.Header().Set("Allow", "POST")
			problem(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if h.draining.Load() {
			problem(w, http.StatusServiceUnavailable, "draining")
			return
		}
		select {
		case h.resolutions <- struct{}{}:
			defer func() {
				<-h.resolutions
			}()
		default:
			w.Header().Set("Retry-After", "1")
			problem(w, http.StatusTooManyRequests, "overloaded")
			return
		}
		h.resolve(w, r)
	default:
		problem(w, http.StatusNotFound, "not_found")
	}
}

// Serve owns accepted connections and cancels handlers before joining shutdown.
// The caller closes the engine only after Serve returns.
func Serve(ctx context.Context, listener net.Listener, handler *Handler) error {
	return serveWithDrain(ctx, listener, handler, 5*time.Second)
}

func serveWithDrain(ctx context.Context, listener net.Listener, handler *Handler, drain time.Duration) error {
	if listener == nil || handler == nil {
		return errors.New("listener and handler required")
	}
	if ctx == nil {
		return errors.New("context cannot be nil")
	}
	requestContext, cancel := context.WithCancel(context.Background())
	owned := &handlerOwner{handler: handler}
	defer cancel()
	server := &http.Server{
		Handler: owned,

		ReadHeaderTimeout: 3 * time.Second,

		ReadTimeout: 5 * time.Second,

		WriteTimeout: 35 * time.Second,

		IdleTimeout: 10 * time.Second,

		MaxHeaderBytes: 8 * 1024,

		BaseContext: func(net.Listener) context.Context {
			return requestContext
		},
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		select {
		case <-ctx.Done():
			handler.Drain()
			cancel()
			shutdownContext, stop := context.WithTimeout(context.Background(), drain)
			defer stop()
			if server.Shutdown(shutdownContext) != nil {
				_ = server.Close()
			}
		case <-requestContext.Done():
		}
		// Always close admission and sockets, including listener failure and a
		// forced drain. http.Server.Close alone does not join ServeHTTP calls.
		owned.closeAdmission()
		cancel()
		_ = server.Close()
		owned.handlers.Wait()
	}()
	err := server.Serve(&boundedListener{Listener: listener})
	cancel()
	<-done
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return errors.New("private listener stopped")
}

// handlerOwner closes admission under the same mutex as WaitGroup.Add, so no
// late handler can race Wait after a listener failure or forced socket close.
type handlerOwner struct {
	mu       sync.Mutex
	closed   bool
	handlers sync.WaitGroup
	handler  http.Handler
}

func (o *handlerOwner) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	o.mu.Lock()
	if o.closed {
		o.mu.Unlock()
		problem(w, http.StatusServiceUnavailable, "draining")
		return
	}
	o.handlers.Add(1)
	o.mu.Unlock()
	defer o.handlers.Done()
	o.handler.ServeHTTP(w, r)
}
func (o *handlerOwner) closeAdmission() {
	o.mu.Lock()
	o.closed = true
	o.mu.Unlock()
}

type boundedListener struct {
	net.Listener
	active atomic.Int64
}

func (l *boundedListener) Accept() (net.Conn, error) {
	for {
		connection, err := l.Listener.Accept()
		if err != nil {
			return nil, err
		}
		if l.active.Add(1) > MaxConnections {
			l.active.Add(-1)
			_ = connection.Close()
			continue
		}
		return &boundedConnection{
			Conn: connection,
			release: func() {
				l.active.Add(-1)
			},
		}, nil
	}
}

type boundedConnection struct {
	net.Conn
	once    sync.Once
	release func()
}

func (c *boundedConnection) Close() error {
	err := c.Conn.Close()
	c.once.Do(c.release)
	return err
}
