package network

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"family-stream-providers/internal/model"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"syscall"
	"time"
)

// Guard enforces that the network environment is healthy and attested before traffic egresses.
// In production, the implementation checks Mullvad VPN state and bound interfaces.
type Guard interface {
	Check(ctx context.Context) error
}

// GuardFunc enables standard functions to satisfy the Guard interface.
type GuardFunc func(ctx context.Context) error

func (fn GuardFunc) Check(ctx context.Context) error {
	return fn(ctx)
}

// Options configures a secure, fail-closed HTTP network client.
type Options struct {
	AddressFamily  string // Empty/ip, ip4 or ip6; only diagnostics restrict the family.
	Budget         *AcceptanceBudget
	Guard          Guard
	Cleanup        func()
	Resolver       *net.Resolver
	SocketControl  func(network, address string, c syscall.RawConn) error
	RequestTimeout time.Duration
	MaxBytes       int64
	MaxConnections int
}

// Client provides bounded HTTP transport with tunnel-bound sockets, SSRF defenses, and credential isolation.
type Client struct {
	family   string
	budget   *AcceptanceBudget
	http     *http.Client
	guard    Guard
	resolver *net.Resolver
	dialer   net.Dialer
	timeout  time.Duration
	maxBytes int64
	cleanup  func()
}

// Request specifies parameters for an outbound HTTP request.
type Request struct {
	URL     string
	Method  string
	Body    []byte // Bounded protocol payload; never logged.
	Context model.FetchContext
	Headers map[string]string // Internal controls (e.g. Range, Accept); never unchecked client headers.
	Jar     http.CookieJar    // Per-lookup session cookies; isolated from pooled client state.
	Limit   int64
	Sample  bool // If true, truncate body at Limit without error (useful for media sampling).
}

// Response contains bounded response data and latency measurements.
type Response struct {
	URL       *url.URL
	Status    int
	Header    http.Header
	Body      []byte
	FirstByte time.Duration
	Transfer  time.Duration
}

// New constructs and initializes a secure Client instance.
func New(opts Options) (*Client, error) {
	if opts.AddressFamily != "" && opts.AddressFamily != "ip" && opts.AddressFamily != "ip4" && opts.AddressFamily != "ip6" {
		return nil, errors.New("invalid address family")
	}
	if opts.AddressFamily == "" {
		opts.AddressFamily = "ip"
	}
	if opts.Guard == nil {
		return nil, errors.New("fail-closed VPN guard required")
	}

	if opts.RequestTimeout <= 0 {
		opts.RequestTimeout = 8 * time.Second
	}
	if opts.MaxBytes <= 0 {
		opts.MaxBytes = 5_000_000
	}
	if opts.MaxConnections <= 0 {
		opts.MaxConnections = 16
	}

	client := &Client{
		family:   opts.AddressFamily,
		guard:    opts.Guard,
		budget:   opts.Budget,
		resolver: net.DefaultResolver,
		dialer: net.Dialer{
			Timeout:   3 * time.Second,
			KeepAlive: 30 * time.Second,
		},
		timeout:  opts.RequestTimeout,
		maxBytes: opts.MaxBytes,
		cleanup:  opts.Cleanup,
	}

	if opts.Resolver != nil {
		client.resolver = opts.Resolver
	}
	client.dialer.Control = opts.SocketControl

	// Tune connection pooling for concurrent media probing across CDNs:
	// Increased idle connection limits prevent TCP teardown and TLS re-handshakes over Mullvad.
	transport := &http.Transport{
		Proxy:                  nil,
		DisableCompression:     true,
		MaxResponseHeaderBytes: 16 * 1024,
		DialContext:            client.dial,
		MaxConnsPerHost:        opts.MaxConnections,
		MaxIdleConns:           128,
		MaxIdleConnsPerHost:    32,
		IdleConnTimeout:        60 * time.Second,
		TLSHandshakeTimeout:    3 * time.Second,
		ResponseHeaderTimeout:  5 * time.Second,
		TLSClientConfig: &tls.Config{
			MinVersion: tls.VersionTLS12,
		},
	}

	client.http = &http.Client{
		Transport: transport,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			// Manual redirect handling in Fetch() prevents automatic credential leaking.
			return http.ErrUseLastResponse
		},
	}

	return client, nil
}

// Close closes idle transport connections and invokes any cleanup routines.
func (c *Client) Close() {
	c.http.CloseIdleConnections()
	if c.cleanup != nil {
		c.cleanup()
	}
}

// pathMatches tests if a URL path matches a prefix according to path segment boundaries.
func pathMatches(path, prefix string) bool {
	if prefix == "" || prefix == "/" || path == prefix {
		return true
	}

	if strings.HasPrefix(path, prefix) {
		if strings.HasSuffix(prefix, "/") || strings.HasPrefix(strings.TrimPrefix(path, prefix), "/") {
			return true
		}
	}

	return false
}

// applyContext attaches scoped headers and cookies to an outbound request if the origin and path match.
func applyContext(header http.Header, targetURL *url.URL, fetchContext model.FetchContext) error {
	if publicRes := fetchContext.PublicResources; publicRes != nil {
		for name, value := range map[string]string{"Origin": publicRes.Origin, "Referer": publicRes.Referer} {
			if value == "" {
				continue
			}
			identityURL, err := url.Parse(value)
			if err != nil || ValidateURL(identityURL) != nil || identityURL.RawQuery != "" || identityURL.ForceQuery || identityURL.Fragment != "" || (identityURL.Path != "" && identityURL.Path != "/") {
				return errors.New("public resource identity must be a validated root origin")
			}
			header.Set(name, value)
		}
	}

	for _, scope := range fetchContext.Scopes {
		origin := scopeOrigin(scope.Origin)
		if origin == "" {
			return errors.New("invalid context scope")
		}

		if origin != Origin(targetURL) || !pathMatches(targetURL.Path, scope.PathPrefix) {
			continue
		}

		for name, value := range scope.Headers {
			normalized := strings.ToLower(name)
			switch normalized {
			case "user-agent", "accept", "accept-language", "referer", "origin", "authorization", "x-playback-environment", "x-csrf-token":
				header.Set(name, value)
			default:
				return errors.New("unsupported scoped header")
			}
		}

		if scope.Cookies != "" {
			if scope.CookiePath == "" {
				return errors.New("cookie path required")
			}
			if pathMatches(targetURL.Path, scope.CookiePath) {
				header.Set("Cookie", scope.Cookies)
			}
		}
	}

	return nil
}

// Fetch executes a bounded HTTP request with SSRF validation, origin scoping, and redirect containment.
func (c *Client) fetch(ctx context.Context, req Request) (Response, error) {
	budgetCtx, stopBudget := c.budget.Context(ctx)
	defer stopBudget()
	reqCtx, cancel := context.WithTimeout(budgetCtx, c.timeout)
	defer cancel()

	limit := req.Limit
	if limit <= 0 || limit > c.maxBytes {
		limit = c.maxBytes
	}

	targetURL, err := url.Parse(req.URL)
	if err != nil || ValidateURL(targetURL) != nil {
		return Response{}, errors.New("destination denied")
	}

	method := req.Method
	if method == "" {
		method = http.MethodGet
	}
	if method != http.MethodGet && method != http.MethodPost {
		return Response{}, errors.New("method denied")
	}

	if len(req.Body) > 64*1024 || (method == http.MethodGet && len(req.Body) > 0) {
		return Response{}, errors.New("request body denied")
	}

	initialOrigin := Origin(targetURL)
	startTime := time.Now()

	for redirects := 0; ; redirects++ {
		if err := c.guard.Check(reqCtx); err != nil {
			return Response{}, errors.New("VPN route unavailable")
		}

		httpReq, err := http.NewRequestWithContext(reqCtx, method, targetURL.String(), bytes.NewReader(req.Body))
		if err != nil {
			return Response{}, errors.New("invalid request")
		}

		if err := applyContext(httpReq.Header, targetURL, req.Context); err != nil {
			return Response{}, err
		}

		for name, value := range req.Headers {
			if name != "Range" && name != "Accept" && name != "Content-Type" {
				return Response{}, errors.New("unsupported request control")
			}
			httpReq.Header.Set(name, value)
		}

		if req.Jar != nil && Origin(targetURL) == initialOrigin {
			for _, cookie := range req.Jar.Cookies(targetURL) {
				httpReq.AddCookie(cookie)
			}
		}

		// Charge headers, bounded body, overflow/drain and HTTP read-ahead.
		// TLS/TCP/VPN overhead is explicitly outside this Go allowance.
		if err := c.budget.Take(limit + 1 + 4096 + 16*1024 + 32*1024); err != nil {
			return Response{}, err
		}
		resp, err := c.http.Do(httpReq)
		if err != nil {
			if reqCtx.Err() != nil {
				return Response{}, reqCtx.Err()
			}
			return Response{}, fmt.Errorf("upstream request failed: %w", err)
		}

		if req.Jar != nil && Origin(targetURL) == initialOrigin {
			req.Jar.SetCookies(targetURL, resp.Cookies())
		}

		// Handle HTTP redirects manually to maintain origin isolation.
		switch resp.StatusCode {
		case http.StatusMovedPermanently, http.StatusFound, http.StatusSeeOther, http.StatusTemporaryRedirect, http.StatusPermanentRedirect:
			// Drain small redirect body (up to 4KB) before closing to allow HTTP connection reuse.
			_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
			_ = resp.Body.Close()

			if method != http.MethodGet || redirects >= 3 {
				return Response{}, errors.New("redirect denied")
			}

			location := resp.Header.Get("Location")
			if location == "" {
				return Response{}, errors.New("redirect destination denied")
			}

			nextURL, parseErr := targetURL.Parse(location)
			if parseErr != nil || ValidateURL(nextURL) != nil || (targetURL.Scheme == "https" && nextURL.Scheme != "https") {
				return Response{}, errors.New("redirect destination denied")
			}

			// Destination validated; proceed to next redirect hop.
			targetURL = nextURL
			continue
		}

		// Read response body with first-byte timing and size limits.
		var firstByte [1]byte
		bytesRead, readErr := io.ReadFull(resp.Body, firstByte[:])
		timeToFirstByte := time.Since(startTime)
		transferStartTime := time.Now()

		if readErr != nil && readErr != io.EOF {
			_ = resp.Body.Close()
			if reqCtx.Err() != nil {
				return Response{}, reqCtx.Err()
			}
			return Response{}, errors.New("upstream body failed")
		}

		allowance := limit - int64(bytesRead)
		if !req.Sample {
			// Read one additional byte to detect if the body exceeds the limit.
			allowance++
		}

		remainingBytes, readErr := io.ReadAll(io.LimitReader(resp.Body, allowance))
		if !req.Sample {
			// Drain remaining small payload (up to 4KB) to allow TCP connection reuse.
			_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		}
		_ = resp.Body.Close()

		if readErr != nil {
			if reqCtx.Err() != nil {
				return Response{}, reqCtx.Err()
			}
			return Response{}, errors.New("upstream body failed")
		}

		body := append(firstByte[:bytesRead:bytesRead], remainingBytes...)
		if !req.Sample && int64(len(body)) > limit {
			return Response{}, errors.New("response limit exceeded")
		}

		return Response{
			URL:       targetURL,
			Status:    resp.StatusCode,
			Header:    resp.Header,
			Body:      body,
			FirstByte: timeToFirstByte,
			Transfer:  time.Since(transferStartTime),
		}, nil
	}
}
