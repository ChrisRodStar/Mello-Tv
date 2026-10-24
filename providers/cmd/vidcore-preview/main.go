// Command vidcore-preview hosts a temporary local HTTP preview server allowing playback
// of a resolved selection in macOS Safari for visual inspection.
// Note: This is an internal testing harness, not the production relay.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"html"
	"net"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strings"
	"sync"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/probe"
)

// resourceEntry pairs an upstream resource URL with its functional type (e.g. playlist, segment, subtitle).
type resourceEntry struct {
	URL  string
	Kind string
}

// previewClient abstracts the HTTP transport for testability.
type previewClient interface {
	Fetch(ctx context.Context, req network.Request) (network.Response, error)
}

// previewServer proxies and rewrites HLS streams to localhost for Safari previewing.
type previewServer struct {
	label           string
	rootPath        string
	client          previewClient
	source          model.Candidate
	subtitleHTML    string
	subtitleOptions string
	mu              sync.Mutex
	resources       map[string]resourceEntry
	idLookup        map[string]string
	slots           chan struct{}
}

// register registers an upstream media resource and assigns a local random identifier.
func (p *previewServer) register(rawURL, kind string) (string, error) {
	parsedURL, err := url.Parse(rawURL)
	if err != nil || network.ValidateURL(parsedURL) != nil {
		return "", errors.New("unsafe upstream resource URL")
	}

	cacheKey := kind + ":" + rawURL

	p.mu.Lock()
	defer p.mu.Unlock()

	if existingID, found := p.idLookup[cacheKey]; found {
		return "/resource/" + existingID, nil
	}

	if len(p.resources) >= 5000 {
		return "", fmt.Errorf("preview resource cache capacity limit reached")
	}

	randomBytes := make([]byte, 12)
	if _, err := rand.Read(randomBytes); err != nil {
		return "", err
	}

	generatedID := hex.EncodeToString(randomBytes)
	p.idLookup[cacheKey] = generatedID
	p.resources[generatedID] = resourceEntry{
		URL:  rawURL,
		Kind: kind,
	}

	return "/resource/" + generatedID, nil
}

var (
	hlsURIPattern   = regexp.MustCompile(`URI="([^"]+)"`)
	byteRangeHeader = regexp.MustCompile(`^bytes=\d*-\d*$`)
)

func (p *previewServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// Serve root HTML video player.
	if r.URL.Path == "/" && r.Method == http.MethodGet {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		_, _ = fmt.Fprintf(
			w,
			`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><title>%s</title></head><body style="margin:0;background:#111;color:#eee;font:16px system-ui"><video controls playsinline preload="metadata" style="display:block;width:100%%;height:80vh" src="%s">%s</video><p style="margin:16px">%s — Test playback, seeking, audio and subtitles in Safari.</p><p style="margin:16px"><label>Subtitles: <select id="subtitles"><option value="-1">Off</option>%s</select></label> <span id="subtitle-status"></span></p><script>
const video = document.querySelector('video');
const subtitles = document.getElementById('subtitles');
const status = document.getElementById('subtitle-status');
function selectSubtitles() {
  const selected = Number(subtitles.value);
  const externalTracks = Array.from(video.querySelectorAll('track'));
  for (let index = 0; index < externalTracks.length; index++) {
    externalTracks[index].track.mode = index === selected ? 'showing' : 'disabled';
  }
  const streamSubtitles = Array.from(video.textTracks).some(track =>
    (track.kind === 'subtitles' || track.kind === 'captions') &&
    !externalTracks.some(element => element.track === track));
  status.textContent = selected < 0 && !externalTracks.length ?
    (streamSubtitles ? 'Stream subtitles: use the player menu.' : 'No external subtitle track reported for this source.') : '';
}
subtitles.addEventListener('change', selectSubtitles);
video.addEventListener('loadedmetadata', selectSubtitles);
video.textTracks.addEventListener('addtrack', selectSubtitles);
for (const track of video.querySelectorAll('track')) {
  track.addEventListener('error', () => { status.textContent = 'Subtitle download failed.'; });
}
selectSubtitles();
</script></body></html>`,
			html.EscapeString(p.label),
			html.EscapeString(p.rootPath),
			p.subtitleHTML,
			html.EscapeString(p.label),
			p.subtitleOptions,
		)
		return
	}

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		http.Error(w, "GET required", http.StatusMethodNotAllowed)
		return
	}

	resourceID := strings.TrimPrefix(r.URL.Path, "/resource/")
	p.mu.Lock()
	entry, found := p.resources[resourceID]
	p.mu.Unlock()

	if !found {
		http.NotFound(w, r)
		return
	}

	requestTimeout := 25 * time.Second
	if entry.Kind == "mp4" {
		requestTimeout = 2 * time.Minute
	}
	reqCtx, cancel := context.WithTimeout(r.Context(), requestTimeout)
	defer cancel()

	select {
	case p.slots <- struct{}{}:
		defer func() { <-p.slots }()
	case <-reqCtx.Done():
		http.Error(w, "preview request deadline exceeded", http.StatusGatewayTimeout)
		return
	}

	if entry.Kind == "mp4" {
		p.serveMP4(reqCtx, w, r, entry)
		return
	}
	if r.Method == http.MethodHead {
		http.Error(w, "GET required", http.StatusMethodNotAllowed)
		return
	}

	customHeaders := make(map[string]string)
	if rangeVal := r.Header.Get("Range"); rangeVal != "" {
		if !byteRangeHeader.MatchString(rangeVal) {
			http.Error(w, "invalid byte range specification", http.StatusBadRequest)
			return
		}
		customHeaders["Range"] = rangeVal
	}

	downloadLimit := int64(32 * 1024 * 1024)
	if entry.Kind == "playlist" || entry.Kind == "subtitle-playlist" {
		downloadLimit = 1024 * 1024
	}

	fetchStarted := time.Now()
	resp, err := p.client.Fetch(reqCtx, network.Request{
		URL:     entry.URL,
		Context: p.source.Context,
		Headers: customHeaders,
		Limit:   downloadLimit,
	})
	if err != nil {
		fmt.Printf("Preview resource=%s failure=%s elapsed_ms=%d\n", entry.Kind, model.Category(err), time.Since(fetchStarted).Milliseconds())
		http.Error(w, "upstream fetch failed", http.StatusBadGateway)
		return
	}

	if resp.Status != http.StatusOK && resp.Status != http.StatusPartialContent {
		fmt.Printf("Preview resource=%s upstream_status=%d elapsed_ms=%d\n", entry.Kind, resp.Status, time.Since(fetchStarted).Milliseconds())
		http.Error(w, fmt.Sprintf("upstream returned HTTP %d", resp.Status), http.StatusBadGateway)
		return
	}

	responseBody := resp.Body
	contentType := resp.Header.Get("Content-Type")
	if entry.Kind == "subtitle" {
		if !strings.HasPrefix(strings.TrimPrefix(string(responseBody), "\uFEFF"), "WEBVTT") {
			http.Error(w, "subtitle response is not WebVTT", http.StatusBadGateway)
			return
		}
		contentType = "text/vtt"
	}

	// Rewrite HLS playlists to proxy sub-playlists, audio tracks, keys, and media segments locally.
	if entry.Kind == "playlist" || entry.Kind == "subtitle-playlist" {
		if !strings.HasPrefix(string(responseBody), "#EXTM3U") {
			http.Error(w, "invalid HLS manifest from upstream", http.StatusBadGateway)
			return
		}

		if entry.URL == p.source.URL && p.source.SelectedVariantURL != "" {
			filtered, err := probe.SelectMasterVariant(responseBody, resp.URL, p.source.SelectedVariantURL)
			if err != nil {
				http.Error(w, "selected rendition missing or master changed", http.StatusBadGateway)
				return
			}
			responseBody = filtered
		}

		lines := strings.Split(string(responseBody), "\n")
		isNextVariantPlaylist := false
		isFragmentedMP4 := false
		var rewriteErr error

		rewriteLink := func(rawURI, kind string) string {
			resolved, err := resp.URL.Parse(rawURI)
			if err != nil {
				rewriteErr = err
				return ""
			}
			localPath, err := p.register(resolved.String(), kind)
			if err != nil {
				rewriteErr = err
			}
			return localPath
		}

		for i, line := range lines {
			if strings.HasPrefix(line, "#EXT-X-STREAM-INF:") {
				isNextVariantPlaylist = true
			}
			if strings.HasPrefix(line, "#EXT-X-MAP:") {
				isFragmentedMP4 = true
			}

			if strings.HasPrefix(line, "#") {
				tagKind := "segment"
				if strings.HasPrefix(line, "#EXT-X-KEY:") || strings.HasPrefix(line, "#EXT-X-SESSION-KEY:") {
					tagKind = "key"
				} else if strings.HasPrefix(line, "#EXT-X-MAP:") {
					tagKind = "init"
				} else if strings.HasPrefix(line, "#EXT-X-MEDIA:") || strings.HasPrefix(line, "#EXT-X-I-FRAME-STREAM-INF:") {
					tagKind = "playlist"
					if strings.HasPrefix(line, "#EXT-X-MEDIA:") {
						media, err := probe.ParsePlaylist([]byte("#EXTM3U\n"+line+"\n"), resp.URL)
						if err != nil {
							rewriteErr = err
						} else if len(media.Metadata.Tracks) > 0 && media.Metadata.Tracks[0].Kind == "subtitles" {
							tagKind = "subtitle-playlist"
						}
					}
				}

				lines[i] = hlsURIPattern.ReplaceAllStringFunc(line, func(match string) string {
					submatch := hlsURIPattern.FindStringSubmatch(match)
					return `URI="` + rewriteLink(submatch[1], tagKind) + `"`
				})
			} else if strings.TrimSpace(line) != "" {
				segmentKind := "segment"
				if entry.Kind == "subtitle-playlist" {
					segmentKind = "subtitle"
				}
				if isNextVariantPlaylist {
					segmentKind = "playlist"
				} else if isFragmentedMP4 {
					segmentKind = "fragment"
				}
				lines[i] = rewriteLink(strings.TrimSpace(line), segmentKind)
				isNextVariantPlaylist = false
			}
		}

		if rewriteErr != nil {
			http.Error(w, "failed rewriting playlist resources", http.StatusBadGateway)
			return
		}

		responseBody = []byte(strings.Join(lines, "\n"))
		contentType = "application/vnd.apple.mpegurl"
	} else if !strings.HasPrefix(contentType, "video/") && !strings.HasPrefix(contentType, "audio/") {
		switch entry.Kind {
		case "segment":
			contentType = "video/mp2t"
		case "init", "fragment":
			contentType = "video/mp4"
		case "subtitle":
			contentType = "text/vtt"
		case "key":
			contentType = "application/octet-stream"
		}
	}

	w.Header().Set("Content-Type", contentType)
	w.Header().Set("Cache-Control", "private, no-store")

	if entry.Kind != "playlist" && entry.Kind != "subtitle-playlist" {
		for _, headerName := range []string{"Content-Range", "Accept-Ranges"} {
			if headerVal := resp.Header.Get(headerName); headerVal != "" {
				w.Header().Set(headerName, headerVal)
			}
		}
	}

	w.WriteHeader(resp.Status)
	_, _ = w.Write(responseBody)
}

func main() {
	selectionPath := flag.String("selection", "", "path to selection JSON file")
	backupIndex := flag.Int("backup", -1, "optional existing backup index for an explicitly unverified alternate-source preview")
	port := flag.Int("port", 0, "loopback preview port (zero chooses an available port)")
	label := flag.String("label", "VidCore preview", "title shown on the local playback page")
	flag.Parse()

	if *port < 0 || *port > 65535 {
		fmt.Fprintln(os.Stderr, "Preview port must be between 0 and 65535")
		os.Exit(1)
	}
	if *selectionPath == "" {
		_, _ = fmt.Fprintln(os.Stderr, "Usage: vidcore-preview -selection <path-to-selection.json>")
		os.Exit(1)
	}

	data, err := os.ReadFile(*selectionPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to read selection file: %v\n", err)
		os.Exit(1)
	}

	var selectionResult model.Selection
	if err := json.Unmarshal(data, &selectionResult); err != nil || selectionResult.Selected == nil {
		fmt.Fprintf(os.Stderr, "Invalid selection file: selected stream missing\n")
		os.Exit(1)
	}

	if *backupIndex >= 0 {
		if *backupIndex >= len(selectionResult.Backups) {
			fmt.Fprintln(os.Stderr, "Backup index is outside the saved selection")
			os.Exit(1)
		}
		selectionResult.Selected = &selectionResult.Backups[*backupIndex]
	}

	opts, err := network.LocalMullvadOptions(context.Background(), "1.1.1.1:53")
	if err != nil {
		fmt.Fprintf(os.Stderr, "Local Mullvad VPN configuration error: %v\n", err)
		os.Exit(1)
	}

	opts.RequestTimeout = 25 * time.Second
	opts.MaxBytes = 32 * 1024 * 1024

	client, err := network.New(opts)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Network client initialization failed: %v\n", err)
		os.Exit(1)
	}
	defer client.Close()

	srv := &previewServer{
		label:     *label,
		client:    client,
		source:    selectionResult.Selected.Candidate.Source,
		resources: make(map[string]resourceEntry),
		idLookup:  make(map[string]string),
		slots:     make(chan struct{}, 4),
	}

	rootKind := "playlist"
	if srv.source.Format == model.MP4 {
		rootKind = "mp4"
	}
	rootPath, err := srv.register(srv.source.URL, rootKind)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to register root stream playlist: %v\n", err)
		os.Exit(1)
	}
	srv.rootPath = rootPath

	subtitleIndex := 0
	englishDefaultSet := false
	for _, track := range srv.source.Metadata.Tracks {
		if track.Kind != "subtitles" || track.URL == "" {
			continue
		}
		localPath, err := srv.register(track.URL, "subtitle")
		if err != nil {
			fmt.Fprintln(os.Stderr, "Failed to register subtitle resource")
			os.Exit(1)
		}
		defaultAttribute := ""
		selectedAttribute := ""
		if track.Language == "en" && !englishDefaultSet {
			defaultAttribute = " default"
			selectedAttribute = " selected"
			englishDefaultSet = true
		}
		srv.subtitleHTML += fmt.Sprintf(
			`<track kind="subtitles" src="%s" label="%s" srclang="%s"%s>`,
			html.EscapeString(localPath),
			html.EscapeString(track.Name),
			html.EscapeString(track.Language),
			defaultAttribute,
		)
		srv.subtitleOptions += fmt.Sprintf(
			`<option value="%d"%s>%s</option>`,
			subtitleIndex,
			selectedAttribute,
			html.EscapeString(track.Name),
		)
		subtitleIndex++
	}

	listener, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", *port))
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to open local listener: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Local preview player running at: http://%s/\n", listener.Addr())

	httpServer := &http.Server{
		Handler:           srv,
		ReadHeaderTimeout: 5 * time.Second,
	}

	// Terminate preview automatically after 30 minutes.
	time.AfterFunc(30*time.Minute, func() {
		_ = httpServer.Close()
	})

	if err := httpServer.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
		_, _ = fmt.Fprintln(os.Stderr, "Preview server terminated:", err)
	}
}
