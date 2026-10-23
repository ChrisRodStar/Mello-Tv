package api

import (
	"encoding/json"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestCompiledDaemonLifecycle(t *testing.T) {
	binary := os.Getenv("FAMILY_STREAM_API_BINARY")
	if binary == "" {
		t.Skip("explicit built-binary smoke")
	}
	f := newFixture(t)
	f.server.Close()
	f.s.Close()
	listener, e := net.Listen("tcp", "127.0.0.1:0")
	if e != nil {
		t.Fatal(e)
	}
	f.c.Listen = listener.Addr().String()
	listener.Close()
	f.c.Origin = "http://" + f.c.Listen
	config := filepath.Join(t.TempDir(), "api.json")
	if e = os.WriteFile(config, marshal(f.c), 0600); e != nil {
		t.Fatal(e)
	}
	client := &http.Client{Timeout: time.Second}
	var child *exec.Cmd
	stop := func() {
		t.Helper()
		if child == nil {
			return
		}
		start := time.Now()
		_ = child.Process.Signal(os.Interrupt)
		done := make(chan error, 1)
		go func() { done <- child.Wait() }()
		select {
		case e := <-done:
			if e != nil {
				t.Error("daemon failed graceful exit")
			}
		case <-time.After(5 * time.Second):
			_ = child.Process.Kill()
			<-done
			t.Error("daemon did not join")
		}
		if time.Since(start) > 5*time.Second {
			t.Error("drain exceeded bound")
		}
		child = nil
	}
	defer stop()
	start := func() {
		t.Helper()
		child = exec.Command(binary, config)
		child.Env = []string{"PATH=" + os.Getenv("PATH"), "TMPDIR=" + os.TempDir()}
		child.Stdout = io.Discard
		child.Stderr = io.Discard
		if e = child.Start(); e != nil {
			t.Fatal(e)
		}
		deadline := time.Now().Add(5 * time.Second)
		for {
			response, e := client.Get(f.c.Origin + "/health/readyz")
			if e == nil {
				response.Body.Close()
				if response.StatusCode == 200 {
					return
				}
			}
			if time.Now().After(deadline) {
				t.Fatal("daemon readiness timeout")
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	start()
	f.server.URL = f.c.Origin
	token := f.setup()
	p := f.profile(token)
	f.want(204, "PUT", "/api/v1/profiles/"+p.ID+"/library/movie/7", token, nil, "")
	stop()
	start()
	b := f.want(200, "GET", "/api/v1/profiles/"+p.ID, token, nil, "")
	var got Profile
	if json.Unmarshal(b, &got) != nil || got.ID != p.ID {
		t.Fatal("compiled restart lost profile")
	}
	f.want(200, "GET", "/api/v1/profiles/"+p.ID+"/library", token, nil, "")
	stop()
	// Restore the in-process owner so the shared fixture cleanup remains ordinary.
	f.open()
}
