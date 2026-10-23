package vidsrc

import (
	"encoding/json"
	"os"
	"reflect"
	"testing"
)

func fixture(t *testing.T, name string) []byte {
	t.Helper()
	body, err := os.ReadFile("../../../testdata/providers/vidsrc/" + name)
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func TestNativeDecryptGolden(t *testing.T) {
	var envelope struct {
		Data struct {
			Streams string `json:"stream_urls"`
		} `json:"data"`
	}
	if err := json.Unmarshal(fixture(t, "encrypted.json"), &envelope); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"chacha-layout.wasm", "rotating-layout.wasm", "shifted-functions.wasm"} {
		t.Run(name, func(t *testing.T) {
			streams, err := decryptStreams(envelope.Data.Streams, fixture(t, name))
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(streams, []string{"https://cdn.example/a.m3u8", "https://cdn.example/b.m3u8"}) {
				t.Fatal("native cipher disagrees with independent zero-key/nonce ChaCha20 vector")
			}
		})
	}
}

func TestRejectUnknownStaticCipher(t *testing.T) {
	module := fixture(t, "chacha-layout.wasm")
	for size := 0; size < len(module); size++ {
		if _, err := moduleKey(module[:size]); err == nil {
			t.Fatalf("accepted truncated module at %d", size)
		}
	}
	changed := append([]byte(nil), module...)
	// This lies inside the pinned ChaCha20 block body, not the changing data.
	changed[300] ^= 1
	if _, err := moduleKey(changed); err == nil {
		t.Fatal("accepted changed cipher")
	}
	if _, err := moduleKey(append(module, 0)); err == nil {
		t.Fatal("accepted trailing section")
	}
}

// Private static replay never contacts the network or executes captured code.
func TestPrivateCapture(t *testing.T) {
	directory := os.Getenv("VIDSRC_CAPTURE")
	if directory == "" {
		t.Skip("private capture not provided")
	}
	for _, kind := range []string{"movie", "tv"} {
		body, err := os.ReadFile(directory + "/" + kind + "-api.json")
		if err != nil {
			t.Fatal(err)
		}
		var envelope struct {
			Data struct {
				Streams string `json:"stream_urls"`
			} `json:"data"`
		}
		if err := json.Unmarshal(body, &envelope); err != nil {
			t.Fatal(err)
		}
		module, err := os.ReadFile(directory + "/decrypt.wasm")
		if err != nil {
			t.Fatal(err)
		}
		streams, err := decryptStreams(envelope.Data.Streams, module)
		if err != nil {
			t.Fatal(err)
		}
		t.Logf("%s decrypted streams=%d", kind, len(streams))
	}
}
