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
	for _, directive := range []string{"no-store", "no-cache", "must-revalidate", "max-age=0", "max-age=invalid"} {
		header := make(http.Header)
		header.Set("Cache-Control", directive)
		if AssetLifetime(header, time.Minute) != 0 {
			t.Fatalf("uncacheable static response with %q retained", directive)
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

	// Nil header has no cookies or cache-control, returns maximum lifetime
	if AssetLifetime(nil, time.Minute) != time.Minute {
		t.Fatal("nil header should return maximum lifetime")
	}

	// Nil Values receiver safety
	var nilValues *Values[int]
	if _, ok := nilValues.Get("key"); ok {
		t.Fatal("nil Values should return false from Get")
	}
	nilValues.Put("key", 1, time.Minute, 10)
	nilValues.Delete("key")
	if nilValues.Len() != 0 {
		t.Fatal("nil Values should return 0 from Len")
	}
}

