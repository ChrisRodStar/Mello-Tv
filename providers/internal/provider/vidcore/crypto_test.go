package vidcore

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"encoding/base64"
	"encoding/binary"
	"stv-providers/internal/model"
	"testing"
)

func TestResponseEnvelopeAuthenticationAndBounds(t *testing.T) {
	p := &protocol{ResponseSeed: bytes.Repeat([]byte{9}, 32), ResponseNumbers: []uint64{123, 456}}
	seed := append([]byte(nil), p.ResponseSeed...)
	for _, n := range p.ResponseNumbers {
		b := make([]byte, 8)
		binary.LittleEndian.PutUint64(b, n)
		seed = append(seed, b...)
	}
	salt := bytes.Repeat([]byte{3}, 16)
	nonce := bytes.Repeat([]byte{4}, 12)
	key := digest(digest(seed), salt)
	block, e := aes.NewCipher(key)
	if e != nil {
		t.Fatal(e)
	}
	gcm, e := cipher.NewGCM(block)
	if e != nil {
		t.Fatal(e)
	}
	expected := []byte(`{"url":"https://example.com/fixture.m3u8"}`)
	plain := append(make([]byte, 8), expected...)
	envelope := concatenate(salt, nonce, gcm.Seal(nil, nonce, plain, nil))
	body := []byte(base64.StdEncoding.EncodeToString(envelope))
	decoded, e := p.decodeResponse(body)
	if e != nil || !bytes.Equal(decoded, expected) {
		t.Fatal("authenticated envelope failed")
	}
	envelope[len(envelope)-1] ^= 1
	if _, e := p.decodeResponse([]byte(base64.StdEncoding.EncodeToString(envelope))); e == nil {
		t.Fatal("tampered response accepted")
	}
	for _, bad := range []string{"", "not base64", base64.StdEncoding.EncodeToString(make([]byte, 43))} {
		if _, e := p.decodeResponse([]byte(bad)); e == nil {
			t.Fatal("malformed envelope accepted")
		}
	}
}
func TestPermutationBoundsAndUniqueness(t *testing.T) {
	for _, n := range []int{1, 16, 256, 513} {
		order := permutation(n, []byte("synthetic seed"))
		seen := map[int]bool{}
		for _, v := range order {
			if v < 0 || v >= n || seen[v] {
				t.Fatal("invalid permutation")
			}
			seen[v] = true
		}
	}
}
func TestSourceContextAndUnknownMetadata(t *testing.T) {
	title := model.Title{Type: model.Movie, TMDBID: 1}
	c, e := normalizeSource([]byte(`{"url":"https://example.com/master.m3u8","noReferrer":true,"tracks":[],"title":"Synthetic title"}`), title, "Supreme")
	if e != nil {
		t.Fatal(e)
	}
	if c.Context.Scopes[0].Headers["Referer"] != "" || c.Metadata.ProviderData["title"] == nil || c.Metadata.TracksKnown || c.ExpiresAt != nil {
		t.Fatal("source context or unknown metadata changed")
	}
	if _, e := normalizeSource([]byte(`{"url":"http://127.0.0.1/media"}`), title, "Supreme"); e == nil {
		t.Fatal("unsafe source accepted")
	}
}
