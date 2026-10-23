package cache

import (
	"net/http"
	"sync"
	"testing"
	"time"
)

func TestValuesBoundsExpiryAndConcurrentAccess(t *testing.T) {
	var values Values[int]
	values.Put("expired", 1, time.Minute, 2)
	values.entries["expired"] = valueEntry[int]{value: 1, expires: time.Now().Add(-time.Second)}
	if _, found := values.Get("expired"); found {
		t.Fatal("expired protocol retained")
	}
	values.Put("one", 1, time.Minute, 2)
	values.Put("two", 2, time.Minute, 2)
	values.Put("three", 3, time.Minute, 2)
	if len(values.entries) != 2 {
		t.Fatal("protocol capacity not enforced")
	}
	var workers sync.WaitGroup
	for index := 0; index < 8; index++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for iteration := 0; iteration < 100; iteration++ {
				values.Put("shared", iteration, time.Minute, 2)
				values.Get("shared")
			}
		}()
	}
	workers.Wait()
	if len(values.entries) > 2 {
		t.Fatal("concurrent insertion exceeded capacity")
	}
}

func TestAssetReuseRespectsSessionAndCacheDirectives(t *testing.T) {
	for _, directive := range []string{"no-store", "no-cache", "max-age=0", "max-age=invalid"} {
		header := make(http.Header)
		header.Set("Cache-Control", directive)
		if AssetLifetime(header, time.Minute) != 0 {
			t.Fatal("uncacheable static response retained")
		}
	}
	header := make(http.Header)
	header.Set("Cache-Control", "public, max-age=10")
	if AssetLifetime(header, time.Minute) != 10*time.Second {
		t.Fatal("upstream lifetime not honored")
	}
	header.Set("Set-Cookie", "session=fixture")
	if AssetLifetime(header, time.Minute) != 0 {
		t.Fatal("session side effect skipped")
	}
}
