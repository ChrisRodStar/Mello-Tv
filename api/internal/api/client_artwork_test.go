package api

import (
	"bytes"
	"context"
	"testing"
)

func TestSnapshotPersistsMetadataWithoutLegacyArtwork(t *testing.T) {
	fixture := newFixture(t)
	fixture.seedSnapshot("")
	var raw []byte
	if err := fixture.s.db.QueryRow("SELECT data FROM snapshot WHERE id=1").Scan(&raw); err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(raw, []byte(`"kind":"artwork"`)) || bytes.Contains(raw, []byte(`"bytes":`)) {
		t.Fatal("image payload persisted")
	}
	fixture.restart()
	if len(fixture.s.records("artwork")) != 0 {
		t.Fatal("image cache retained after restart")
	}
	token := fixture.setup()
	response := fixture.want(200, "GET", "/api/v1/titles/movie/7", token, nil, "")
	if !bytes.Contains(response, []byte("https://image.tmdb.org/t/p/w500/example.png")) {
		t.Fatal("metadata lacks direct image URL")
	}
	if err := fixture.s.SyncSnapshot(context.Background()); err != nil {
		t.Fatal(err)
	}
}
