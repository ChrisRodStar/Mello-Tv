package api

import (
	"encoding/json"
	"testing"
	"time"
)

func TestProvisionalStartupRepliesBeforeBackupEnrichment(t *testing.T) {
	fixture := newFixture(t)
	release := make(chan struct{})
	fixture.mu.Lock()
	fixture.pendingSelection = true
	fixture.completeRelease = release
	fixture.completeStarted = make(chan struct{}, 1)
	fixture.backupEnriched = make(chan struct{}, 1)
	fixture.mu.Unlock()
	profile := fixture.profile("")
	response := fixture.want(200, "POST", "/api/v1/profiles/"+profile.ID+"/playback", "", map[string]any{"type": "movie", "id": 7}, "startup-key")
	var reply struct {
		Revision uint64 `json:"revision"`
	}
	if json.Unmarshal(response, &reply) != nil || reply.Revision != 1 {
		t.Fatal("startup reply did not preserve control revision")
	}
	select {
	case <-fixture.completeStarted:
	case <-time.After(2 * time.Second):
		t.Fatal("background completion request missing")
	}
	select {
	case <-fixture.backupEnriched:
		t.Fatal("unverified backup snapshot imported early")
	default:
	}
	close(release)
	select {
	case <-fixture.backupEnriched:
	case <-time.After(2 * time.Second):
		t.Fatal("completed verified backups not imported")
	}
}

func TestClosingPlaybackCancelsOwnedBackupCompletion(t *testing.T) {
	fixture := newFixture(t)
	release := make(chan struct{})
	fixture.mu.Lock()
	fixture.pendingSelection = true
	fixture.completeRelease = release
	fixture.completeStarted = make(chan struct{}, 1)
	fixture.backupEnriched = make(chan struct{}, 1)
	fixture.mu.Unlock()
	profile := fixture.profile("")
	response := fixture.want(200, "POST", "/api/v1/profiles/"+profile.ID+"/playback", "", map[string]any{"type": "movie", "id": 7}, "cancel-startup")
	var reply struct {
		ID string `json:"id"`
	}
	if json.Unmarshal(response, &reply) != nil {
		t.Fatal("invalid create reply")
	}
	select {
	case <-fixture.completeStarted:
	case <-time.After(2 * time.Second):
		t.Fatal("completion not started")
	}
	fixture.want(204, "DELETE", "/api/v1/profiles/"+profile.ID+"/playback/"+reply.ID, "", nil, "")
	deadline := time.Now().Add(2 * time.Second)
	for {
		fixture.s.lifecycleMu.Lock()
		pending := len(fixture.s.enrichmentCancels)
		fixture.s.lifecycleMu.Unlock()
		if pending == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("closed playback retained background completion")
		}
		time.Sleep(time.Millisecond)
	}
	select {
	case <-fixture.backupEnriched:
		t.Fatal("closed playback received backups")
	default:
	}
}
