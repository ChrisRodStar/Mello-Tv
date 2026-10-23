package vidfast

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"testing"
)

func TestIndependentRequestVector(t *testing.T) {
	encoded, err := os.ReadFile("../../../testdata/providers/vidfast/codec.json")
	if err != nil {
		t.Fatal(err)
	}
	var vector struct {
		Challenge      string   `json:"challenge"`
		ChallengeToken string   `json:"challenge_token"`
		Token          string   `json:"token"`
		Salt           string   `json:"salt"`
		Timestamp      int64    `json:"timestamp"`
		Packet         string   `json:"packet"`
		Packed         string   `json:"packed"`
		ResponseKey    string   `json:"response_key"`
		PRNG           []uint32 `json:"prng"`
	}
	if err := json.Unmarshal(encoded, &vector); err != nil {
		t.Fatal(err)
	}
	transformed, err := challengeToken([]byte(vector.Challenge))
	if err != nil || transformed != vector.ChallengeToken {
		t.Fatalf("independent challenge transformation vector mismatch: %v", err)
	}

	salt, _ := hex.DecodeString(vector.Salt)
	packed, err := packRequest(vector.Token, salt, vector.Timestamp)
	if err != nil || packed != vector.Packed {
		t.Fatalf("independent Python/OpenSSL request vector mismatch: %v", err)
	}
	packet, err := base64.NewEncoding(requestAlphabet).WithPadding(base64.NoPadding).DecodeString(packed)
	if err != nil || hex.EncodeToString(packet) != vector.Packet {
		t.Fatal("binary request framing differs from vector")
	}
	if hex.EncodeToString(responseKey()) != vector.ResponseKey {
		t.Fatal("independent response key derivation mismatch")
	}
	next := splitPRNG([]byte("fixture seed"))
	for _, expected := range vector.PRNG {
		if next() != expected {
			t.Fatal("independent PRNG vector mismatch")
		}
	}
}

func TestRC4PublishedVector(t *testing.T) {
	// The conventional Key/Plaintext RC4 vector independently exercises KSA/PRGA.
	if hex.EncodeToString(rc4Bytes([]byte("Key"), []byte("Plaintext"))) != "bbf316e8d940af0ad3" {
		t.Fatal("RC4 known-answer mismatch")
	}
}

func TestResponseAuthenticationAndBounds(t *testing.T) {
	baseKey := responseKey()
	salt := make([]byte, 16)
	nonce := make([]byte, 12)
	derived := sha256.Sum256(append(append([]byte(nil), baseKey...), salt...))
	block, err := aes.NewCipher(derived[:])
	if err != nil {
		t.Fatal(err)
	}
	mode, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatal(err)
	}
	packet := append(append(salt, nonce...), mode.Seal(nil, nonce, []byte("12345678[]"), nil)...)
	encoded := base64.StdEncoding.EncodeToString(packet)
	plaintext, err := decryptEnvelope(encoded, baseKey)
	if err != nil || string(plaintext) != "[]" {
		t.Fatalf("authenticated response rejected: %v", err)
	}
	packet[len(packet)-1] ^= 1
	if _, err := decryptEnvelope(base64.StdEncoding.EncodeToString(packet), baseKey); err == nil {
		t.Fatal("tampered response authenticated")
	}
	for _, encoded := range []string{
		"",
		"not base64",
		base64.StdEncoding.EncodeToString(make([]byte, 43)),
	} {
		if _, err := decryptEnvelope(encoded, baseKey); err == nil {
			t.Fatal("malformed envelope accepted")
		}
	}
}
