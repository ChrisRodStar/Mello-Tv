package vidcore

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"encoding/base64"
	"net/http/cookiejar"
	"net/url"
	"strings"
	"testing"
	"time"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

type protocolFixture func(context.Context, network.Request) (network.Response, error)

func (fetch protocolFixture) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	return fetch(ctx, request)
}

func sealProtocolFixture(t *testing.T, proto *protocol, plaintext string) []byte {
	t.Helper()
	salt := bytes.Repeat([]byte{3}, 16)
	nonce := bytes.Repeat([]byte{4}, 12)
	block, err := aes.NewCipher(digest(digest(proto.ResponseSeed), salt))
	if err != nil {
		t.Fatal(err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatal(err)
	}
	framed := append(make([]byte, 8), []byte(plaintext)...)
	envelope := concatenate(salt, nonce, gcm.Seal(nil, nonce, framed, nil))
	return []byte(base64.StdEncoding.EncodeToString(envelope))
}

func TestCachedPlayerSkipsFrameworkChunksAndParallelUnlocksJoin(t *testing.T) {
	proto := &protocol{
		Base: "/fixture", ChallengeAction: "challenge", ListAction: "list", UnlockAction: "unlock",
		Key: bytes.Repeat([]byte{1}, 32), IV: bytes.Repeat([]byte{2}, 16),
		ResponseSeed: bytes.Repeat([]byte{3}, 32), Alphabet: make(map[byte]byte),
	}
	for _, symbol := range []byte("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_") {
		proto.Alphabet[symbol] = symbol
	}
	for index := range proto.RC4Keys {
		proto.RC4Keys[index] = []byte("fixture")
		proto.Stages[index].Key = make([]byte, 32)
	}
	list := sealProtocolFixture(t, proto, `[{"name":"first","data":"one"},{"name":"second","data":"two"},{"name":"third","data":"three"}]`)
	source := sealProtocolFixture(t, proto, `{"url":"https://media.example/master.m3u8","noReferrer":true}`)
	started := make(chan struct{})
	release := make(chan struct{})
	jars := make(chan *cookiejar.Jar, 1)
	client := protocolFixture(func(ctx context.Context, request network.Request) (network.Response, error) {
		parsedURL, _ := url.Parse(request.URL)
		response := network.Response{URL: parsedURL, Status: 200}
		switch {
		case parsedURL.Path == "/movie/27205":
			jars <- request.Jar.(*cookiejar.Jar)
			response.Body = []byte(`\"en\":\"fresh-session\" /_next/static/chunks/framework.js /_next/static/chunks/player.js`)
		case strings.HasPrefix(parsedURL.Path, "/fixture/challenge/"):
			response.Body = []byte("challenge-response")
		case strings.HasPrefix(parsedURL.Path, "/fixture/list/"):
			response.Body = list
		case strings.HasPrefix(parsedURL.Path, "/fixture/unlock/"):
			select {
			case started <- struct{}{}:
			case <-ctx.Done():
				return response, ctx.Err()
			}
			select {
			case <-release:
			case <-ctx.Done():
				return response, ctx.Err()
			}
			response.Body = source
		default:
			return response, model.Fail(model.ProtocolChange, "fixture", "unnecessary static chunk request")
		}
		return response, nil
	})
	adapter := &Adapter{client: client}
	adapter.protocols.Put(origin+"/_next/static/chunks/player.js", proto, time.Minute, 8)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	type outcome struct {
		result model.AdapterResult
		err    error
	}
	done := make(chan outcome, 1)
	go func() {
		result, err := adapter.Resolve(ctx, model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
		done <- outcome{result: result, err: err}
	}()
	for range 3 {
		select {
		case <-started:
		case result := <-done:
			t.Fatalf("resolution ended before independent unlocks started: %v", result.err)
		case <-ctx.Done():
			<-done
			t.Fatal("server unlocks did not run concurrently")
		}
	}
	close(release)
	result := <-done
	if result.err != nil || len(result.result.Candidates) != 3 {
		t.Fatalf("parallel unlock failed: %v", result.err)
	}
	for index, name := range []string{"first", "second", "third"} {
		if result.result.Candidates[index].Server != name {
			t.Fatal("parallel completion reordered upstream candidates")
		}
	}
	if <-jars == nil {
		t.Fatal("cached protocol skipped fresh session")
	}
}
