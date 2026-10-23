package vidlink

import (
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strconv"
	"time"

	"golang.org/x/crypto/nacl/secretbox"
)

// This is public player material embedded in the recognized fu.wasm, not a user
// credential. The WASM digest is checked before using its reconstructed algorithm.
const publicTokenKeyHex = "c75136c5668bbfe65a7ecad431a745db68b5f381555b38d8f6c699449cf11fcd"

// generateToken reproduces main.generateToken and main.base64URLEncode from the
// statically disassembled player, without executing WASM or JavaScript:
//  1. Append an unsigned, big-endian Unix expiry (now + 120 seconds) to UTF-8 TMDB ID.
//  2. Use the player's public 32-byte key directly; no KDF, salt or rounds exist.
//  3. XSalsa20-Poly1305 secretbox produces a 16-byte MAC followed by ciphertext.
//  4. Prefix the player's fixed 24-byte zero nonce; encode unpadded base64url.
//
// The fixed nonce is required for compatibility with this public token protocol.
// It is not suitable for encrypting private data and must not be reused elsewhere.
func generateToken(tmdbID int64, now time.Time) (string, error) {
	keyBytes, err := hex.DecodeString(publicTokenKeyHex)
	if err != nil {
		return "", fmt.Errorf("decode public player key: %w", err)
	}

	var key [32]byte
	copy(key[:], keyBytes)

	var nonce [24]byte
	message := append([]byte(strconv.FormatInt(tmdbID, 10)), make([]byte, 8)...)
	binary.BigEndian.PutUint64(message[len(message)-8:], uint64(now.Add(120*time.Second).Unix()))
	sealed := secretbox.Seal(nonce[:], message, &nonce, &key)

	return base64.RawURLEncoding.EncodeToString(sealed), nil
}
