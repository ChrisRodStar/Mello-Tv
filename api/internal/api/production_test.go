package api

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestProductionListeners(t *testing.T) {
	control := os.Getenv("FAMILY_STREAM_RELAY_CONTROL")
	if control == "" {
		t.Skip("explicit tools/production_listeners.py fixture")
	}
	f := newFixture(t)
	f.server.Close()
	f.s.Close()
	f.c.RelayURL = control
	if e := os.WriteFile(f.c.RelayTokenFile, []byte("synthetic-control-01234567890123456789"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(f.c.ProviderTokenFile, []byte("synthetic-service-01234567890123456789"), 0600); e != nil {
		t.Fatal(e)
	}
	readyFile := filepath.Join(t.TempDir(), "ready")
	var provider *exec.Cmd
	startProvider := func() {
		t.Helper()
		_ = os.Remove(readyFile)
		provider = exec.Command(os.Getenv("FAMILY_STREAM_PROVIDER_TEST_BINARY"), "-test.run=^TestAcceptanceHTTPService$")
		provider.Env = append(os.Environ(), "FAMILY_STREAM_SYNTHETIC_READY="+readyFile)
		if e := provider.Start(); e != nil {
			t.Fatal(e)
		}
		deadline := time.Now().Add(5 * time.Second)
		for {
			if b, e := os.ReadFile(readyFile); e == nil {
				var v struct {
					Port int `json:"port"`
				}
				if json.Unmarshal(b, &v) == nil && v.Port > 0 {
					f.c.ProviderURL = "http://127.0.0.1:" + strconv.Itoa(v.Port)
					return
				}
			}
			if time.Now().After(deadline) {
				t.Fatal("provider startup timeout")
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	stopProvider := func() {
		t.Helper()
		if provider != nil {
			_ = provider.Process.Signal(os.Interrupt)
			done := make(chan error, 1)
			go func() { done <- provider.Wait() }()
			select {
			case e := <-done:
				if e != nil {
					t.Error("provider exit rejected")
				}
			case <-time.After(5 * time.Second):
				_ = provider.Process.Kill()
				<-done
				t.Error("provider failed to join")
			}
			provider = nil
		}
	}
	startProvider()
	defer stopProvider()
	f.open()
	token := f.setup()
	p := f.profile(token)
	format := os.Getenv("FAMILY_STREAM_MEDIA_FORMAT")
	request := map[string]any{"type": "tv", "id": 1429, "season": 0, "episode": 1, "imdb_id": "tt2560140", "mal_id": 16498, "device": map[string]any{"formats": []string{format}, "allow_unknown_codecs": true, "allow_unknown_resolution": true}}
	pb := f.play(token, p.ID, request, "production-create")
	path := "/api/v1/profiles/" + p.ID + "/playback/" + pb
	var current map[string]any
	decodeReply := func(b []byte) {
		if e := json.Unmarshal(b, &current); e != nil {
			t.Fatal(e)
		}
	}
	decodeReply(f.want(200, "GET", path, token, nil, ""))
	stream := os.Getenv("FAMILY_STREAM_RELAY_STREAM")
	client := &http.Client{Timeout: 3 * time.Second}
	fetch := func(url string, allowed int) []byte {
		t.Helper()
		r, e := client.Get(stream + url)
		if e != nil {
			t.Fatal(e)
		}
		defer r.Body.Close()
		b, e := io.ReadAll(io.LimitReader(r.Body, 1<<20))
		if e != nil || r.StatusCode != allowed {
			t.Fatalf("production stream status %d expected %d", r.StatusCode, allowed)
		}
		return b
	}
	var firstURL string
	check := func() {
		t.Helper()
		url := current["url"].(string)
		if firstURL == "" {
			firstURL = url
		}
		b := fetch(url, 200)
		if format == "hls" {
			if !bytes.Contains(b, []byte("EXT-X-MEDIA")) {
				t.Fatal("master rendition association lost")
			}
			for _, line := range strings.Split(string(b), "\n") {
				var uri string
				if strings.HasPrefix(line, "/stream/") {
					uri = line
				} else if _, tail, ok := strings.Cut(line, `URI="`); ok {
					uri = strings.Split(tail, `"`)[0]
				}
				if uri != "" {
					fetch(uri, 200)
				}
			}
		} else {
			r, _ := http.NewRequest("GET", stream+url, nil)
			r.Header.Set("Range", "bytes=0-15")
			resp, e := client.Do(r)
			if e != nil {
				t.Fatal(e)
			}
			resp.Body.Close()
			if resp.StatusCode != 206 {
				t.Fatal("MP4 range failed")
			}
		}
	}
	check()
	stopProvider()
	check()
	f.restart()
	check()
	// The Rust owner stops/joins the original store, reopens its encrypted SQLite,
	// and starts fresh production control/stream listeners on the same addresses.
	mailbox := os.Getenv("FAMILY_STREAM_RESTART_MAILBOX")
	if e := os.WriteFile(mailbox, []byte("restart"), 0600); e != nil {
		t.Fatal(e)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, e := os.Stat(mailbox); os.IsNotExist(e) {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("relay restart timeout")
		}
		time.Sleep(10 * time.Millisecond)
	}
	decodeReply(f.want(200, "GET", path, token, nil, ""))
	check()
	startProvider()
	f.server.Close()
	f.s.Close()
	f.open()
	revision := current["revision"].(float64)
	decodeReply(f.want(200, "POST", path+"/backup", token, map[string]any{"expected_revision": revision}, "production-backup"))
	if current["reload_required"] != true {
		t.Fatal("missing reload instruction")
	}
	check()
	revision = current["revision"].(float64)
	decodeReply(f.want(200, "POST", path+"/refresh", token, map[string]any{"expected_revision": revision}, "production-refresh"))
	check()
	// Ensure the lease extension moves beyond the original whole-second expiry.
	time.Sleep(1100 * time.Millisecond)
	revision = current["revision"].(float64)
	decodeReply(f.want(200, "POST", path+"/renew", token, map[string]any{"expected_revision": revision}, "production-renew"))
	check()
	f.want(204, "DELETE", path, token, nil, "")
	r, e := client.Get(stream + firstURL)
	if e != nil {
		t.Fatal(e)
	}
	r.Body.Close()
	if r.StatusCode == 200 {
		t.Fatal("revoked production grant accepted")
	}
}
