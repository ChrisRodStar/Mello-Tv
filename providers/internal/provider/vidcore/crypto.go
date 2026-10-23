package vidcore

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/rc4"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"math/bits"
	"strings"
	"time"
)

// digest computes a single SHA-256 hash across one or more byte slices concatenated together.
func digest(parts ...[]byte) []byte {
	hasher := sha256.New()
	for _, part := range parts {
		hasher.Write(part)
	}
	return hasher.Sum(nil)
}

// concatenate merges multiple byte slices into a single newly allocated slice.
func concatenate(parts ...[]byte) []byte {
	var totalLen int
	for _, part := range parts {
		totalLen += len(part)
	}

	out := make([]byte, 0, totalLen)
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

// randomGenerator creates a deterministic 32-bit xorshift PRNG seeded by the SHA-256 of the input seed.
func randomGenerator(seed []byte) func() uint32 {
	h := digest(seed)
	state := binary.LittleEndian.Uint32(h[0:4]) ^
		binary.LittleEndian.Uint32(h[4:8]) ^
		binary.LittleEndian.Uint32(h[8:12]) ^
		binary.LittleEndian.Uint32(h[12:16])

	if state == 0 {
		state = 0x9e3779b9
	}

	return func() uint32 {
		state ^= state << 13
		state ^= state >> 17
		state ^= state << 5
		return state
	}
}

// permutation generates a Fisher-Yates shuffle permutation of length n using the seeded PRNG.
func permutation(n int, seed []byte) []int {
	out := make([]int, n)
	for i := range out {
		out[i] = i
	}

	nextRand := randomGenerator(seed)
	for i := n - 1; i > 0; i-- {
		j := int(uint64(nextRand()) % uint64(i+1))
		out[i], out[j] = out[j], out[i]
	}

	return out
}

// challenge constructs the obfuscated challenge string required by VidCore's initial handshake.
//
// Pipeline:
//  1. Append 64-bit millisecond timestamp and session token to 16-byte nonce.
//  2. Pad with PKCS#7 to 16-byte boundary and encrypt with AES-256-CBC.
//  3. Apply 32-byte rolling SHA-256 XOR mask.
//  4. Apply byte-wise bitwise rotation and addition based on key-derived rotations.
//  5. Substitute bytes using a 256-entry S-box permutation table.
//  6. Permute 16-byte AES blocks using Fisher-Yates block order.
//  7. Permute individual bytes using byte-level Fisher-Yates positions.
//  8. Append serialized block order, 8-byte SHA-256 checksum, and length header.
//  9. Base64URL encode and substitute characters via the protocol's custom alphabet mapping.
func (p *protocol) challenge(session string, timestamp time.Time, nonce []byte) (string, error) {
	if len(nonce) != 16 || len(session) == 0 || len(session) > 64*1024 {
		return "", protocolError("invalid challenge input")
	}

	stamp := make([]byte, 8)
	binary.LittleEndian.PutUint64(stamp, uint64(timestamp.UnixMilli()))

	plaintext := concatenate(nonce, stamp, []byte(session))

	// Apply standard PKCS#7 padding.
	paddingLen := aes.BlockSize - (len(plaintext) % aes.BlockSize)
	for range paddingLen {
		plaintext = append(plaintext, byte(paddingLen))
	}

	block, err := aes.NewCipher(p.Key)
	if err != nil {
		return "", err
	}

	encrypted := make([]byte, len(plaintext))
	cipher.NewCBCEncrypter(block, p.IV).CryptBlocks(encrypted, plaintext)

	// Step 3: Rolling SHA-256 XOR mask.
	mask := digest(p.Seed, nonce)
	for i := range encrypted {
		if i > 0 && i%32 == 0 {
			mask = digest(mask)
		}
		encrypted[i] ^= mask[i%32]
	}

	// Step 4: Bitwise rotation and constant offset.
	rotations := digest(p.Key, nonce)
	for i := range encrypted {
		rot := rotations[i%32]
		encrypted[i] = bits.RotateLeft8(encrypted[i], int(rot&7)) + (rot ^ 165)
	}

	// Step 5: S-box byte substitution via permutation table.
	sbox := permutation(256, concatenate(nonce, p.Seed, p.IV))
	for i := range encrypted {
		encrypted[i] = byte(sbox[encrypted[i]])
	}

	// Step 6: 16-byte block permutation.
	numBlocks := len(encrypted) / 16
	blockOrder := permutation(numBlocks, concatenate(p.Seed, nonce))
	permutedBlocks := make([]byte, len(encrypted))
	for destIdx, srcIdx := range blockOrder {
		copy(permutedBlocks[destIdx*16:(destIdx+1)*16], encrypted[srcIdx*16:(srcIdx+1)*16])
	}

	// Step 7: Byte-level permutation across the entire buffer.
	bytePositions := permutation(len(permutedBlocks), concatenate(p.Key, nonce, []byte{byte(len(permutedBlocks))}))
	for destIdx, srcIdx := range bytePositions {
		encrypted[destIdx] = permutedBlocks[srcIdx]
	}

	// Step 8: Header, block order serialization, and checksum.
	serializedOrder := make([]byte, 4*len(blockOrder))
	for i, blockIdx := range blockOrder {
		binary.LittleEndian.PutUint32(serializedOrder[i*4:], uint32(blockIdx))
	}

	checksum := digest(serializedOrder, encrypted)[:8]

	if len(blockOrder) > 65535 {
		return "", protocolError("challenge block limit exceeded")
	}

	blockCountHeader := make([]byte, 2)
	binary.LittleEndian.PutUint16(blockCountHeader, uint16(len(blockOrder)))

	payload := concatenate([]byte{1}, nonce, blockCountHeader, serializedOrder, encrypted, checksum)
	encoded := []byte(base64.RawURLEncoding.EncodeToString(payload))

	// Step 9: Alphabet substitution.
	for i, ch := range encoded {
		mappedChar, exists := p.Alphabet[ch]
		if !exists {
			return "", protocolError("challenge alphabet mapping incomplete")
		}
		encoded[i] = mappedChar
	}

	return string(encoded), nil
}

// newNonce generates a cryptographically random 16-byte nonce.
func newNonce() ([]byte, error) {
	nonce := make([]byte, 16)
	_, err := rand.Read(nonce)
	return nonce, err
}

// transformByte executes one of the 11 arithmetic and bitwise transforms recovered from the client bytecode.
func transformByte(value, operation byte) byte {
	switch operation {
	case 0:
		return value - 226
	case 1:
		return value - 76
	case 2:
		return bits.RotateLeft8(value, 2)
	case 3:
		return value - 200
	case 4:
		return value ^ 59
	case 5:
		return value - 218
	case 6:
		return bits.RotateLeft8(value, 1)
	case 7:
		return bits.RotateLeft8(value, -2)
	case 8:
		return value + 230
	case 9:
		return bits.RotateLeft8(value, 7)
	case 10:
		return value - 216
	default:
		return value
	}
}

// apply applies a byte transformation stage by interleaving constant prefix bytes and operating on each byte.
func (s byteStage) apply(input []byte) []byte {
	output := make([]byte, 0, len(input)+len(s.Prefix))

	for i, b := range input {
		if i < len(s.Prefix) {
			output = append(output, s.Prefix[i])
		}
		transformed := transformByte(b^s.Key[i%32], s.Operations[i%10])
		output = append(output, transformed)
	}

	return output
}

// ticket derives the upstream authorization ticket from the server's challenge response text.
//
// Pipeline:
//  1. Validate response is strictly ASCII and reverse the code units (mimicking JS split("").reverse()).
//  2. Lowercase hex-encode the reversed bytes.
//  3. Sequentially pass through stages [0, 4, 1, 2, 3], applying prefix interleaving, byte transforms, and RC4 rounds.
//  4. Base64URL-encode the transformed byte stream.
func (p *protocol) ticket(challengeResponse []byte) (string, error) {
	responseCopy := append([]byte(nil), challengeResponse...)

	for _, b := range responseCopy {
		if b > 127 {
			return "", protocolError("non-ASCII challenge response")
		}
	}

	// Reverse bytes in-place.
	for left, right := 0, len(responseCopy)-1; left < right; left, right = left+1, right-1 {
		responseCopy[left], responseCopy[right] = responseCopy[right], responseCopy[left]
	}

	current := []byte(hex.EncodeToString(responseCopy))

	stageOrder := []int{0, 4, 1, 2, 3}
	for _, stageIdx := range stageOrder {
		current = p.Stages[stageIdx].apply(current)

		cipherStream, err := rc4.NewCipher(p.RC4Keys[stageIdx])
		if err != nil {
			return "", err
		}
		cipherStream.XORKeyStream(current, current)
	}

	return base64.RawURLEncoding.EncodeToString(current), nil
}

// decodeResponse decrypts and verifies an authenticated AES-256-GCM response envelope from VidCore.
//
// Envelope format:
//   - Bytes 0-15:  16-byte salt
//   - Bytes 16-27: 12-byte GCM nonce
//   - Bytes 28+:   Authenticated ciphertext + 16-byte GCM auth tag
//   - Decrypted plaintext prefix: First 8 bytes are stripped as discardable header.
func (p *protocol) decodeResponse(body []byte) ([]byte, error) {
	rawString := strings.TrimSpace(string(body))

	decodedEnvelope, err := base64.StdEncoding.DecodeString(rawString)
	if err != nil {
		decodedEnvelope, err = base64.RawStdEncoding.DecodeString(rawString)
	}
	if err != nil || len(decodedEnvelope) < 44 {
		return nil, protocolError("invalid encoded response envelope")
	}

	salt := decodedEnvelope[0:16]
	nonce := decodedEnvelope[16:28]
	ciphertext := decodedEnvelope[28:]

	// Derive master seed from constant response seed and uint64 response numbers.
	seed := append([]byte(nil), p.ResponseSeed...)
	for _, num := range p.ResponseNumbers {
		buf := make([]byte, 8)
		binary.LittleEndian.PutUint64(buf, num)
		seed = append(seed, buf...)
	}

	masterDigest := digest(seed)
	derivedKey := digest(masterDigest, salt)

	block, err := aes.NewCipher(derivedKey)
	if err != nil {
		return nil, err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	plaintext, err := gcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return nil, protocolError("response authentication failed")
	}

	if len(plaintext) < 8 {
		return nil, protocolError("response plaintext prefix missing")
	}

	return plaintext[8:], nil
}
