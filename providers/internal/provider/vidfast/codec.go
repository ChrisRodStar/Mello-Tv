package vidfast

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"math/bits"
	"unicode/utf8"
)

// These algorithms are explicit translations of the recognized bootstrap's
// static operations, not a bytecode evaluator. The normal browser environment
// mask is zero: DOM, screen and user agent exist, webdriver is false, and no
// headless UA marker is present. Nonzero masks deliberately corrupt the codec.
// Only the fingerprinted version and its zero-mask protocol are supported.
const requestAlphabet = "W_Anx1lGLyJEfTOU3Xeqdh58ki9SHjPm4-Yts0rvCbBg6cDRVFzQNoZI2wMapKu7"

// splitPRNG mirrors xorshift32 seeded by the first four little-endian words of
// SHA-256(seed). The zero-state fallback is the declared golden-ratio constant.
func splitPRNG(seed []byte) func() uint32 {
	digest := sha256.Sum256(seed)
	state := binary.LittleEndian.Uint32(digest[:4]) ^ binary.LittleEndian.Uint32(digest[4:8]) ^
		binary.LittleEndian.Uint32(digest[8:12]) ^ binary.LittleEndian.Uint32(digest[12:16])
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

func permutation(length int, seed []byte) []int {
	order := make([]int, length)
	for index := range order {
		order[index] = index
	}
	next := splitPRNG(seed)
	for index := length - 1; index > 0; index-- {
		other := int(next() % uint32(index+1))
		order[index], order[other] = order[other], order[index]
	}
	return order
}

func joined(parts ...[]byte) []byte {
	var result []byte
	for _, part := range parts {
		result = append(result, part...)
	}
	return result
}

func hexLiteral(value string) []byte {
	decoded, _ := hex.DecodeString(value)
	return decoded
}

// packRequest uses PKCS#7 AES-256-CBC over salt || timestampLE64 || en.
// It then applies a chained SHA-256 XOR mask, per-byte rotations/additions,
// a seeded substitution table, 16-byte block shuffling, and byte shuffling.
// The header carries the salt and block order; SHA-256(order || payload)[:8]
// is a corruption checksum, not an authentication guarantee.
func packRequest(token string, salt []byte, timestamp int64) (string, error) {
	if len(salt) != 16 || len(token) == 0 || len(token) > 4096 || timestamp < 0 {
		return "", errors.New("invalid request codec input")
	}
	key := hexLiteral("d9e8463c1896176b48c12fb2a45216013074cb6003894c2aacf320f633cdfea8")
	iv := hexLiteral("48096b61504315115fc8ff0edbe47a92")
	// Buffer.from(hex) ignores the captured odd trailing nibble; the seed is
	// seven bytes, not eight. Preserve that observed behavior explicitly.
	seed := hexLiteral("ffb6b223470f60")
	clockBytes := make([]byte, 8)
	binary.LittleEndian.PutUint64(clockBytes, uint64(timestamp))
	plaintext := joined(salt, clockBytes, []byte(token))
	padding := aes.BlockSize - len(plaintext)%aes.BlockSize
	for index := 0; index < padding; index++ {
		plaintext = append(plaintext, byte(padding))
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return "", errors.New("invalid request cipher material")
	}
	payload := make([]byte, len(plaintext))
	cipher.NewCBCEncrypter(block, iv).CryptBlocks(payload, plaintext)

	mask := sha256.Sum256(joined(seed, salt))
	for index := range payload {
		if index > 0 && index%32 == 0 {
			mask = sha256.Sum256(mask[:])
		}
		payload[index] ^= mask[index%32]
	}
	rotationMask := sha256.Sum256(joined(key, salt))
	for index, value := range payload {
		maskByte := rotationMask[index%32]
		payload[index] = bits.RotateLeft8(value, int(maskByte&7)) + (maskByte ^ 165)
	}

	substitutions := permutation(256, joined(salt, seed, iv))
	for index, value := range payload {
		payload[index] = byte(substitutions[int(value)])
	}
	blockOrder := permutation(len(payload)/aes.BlockSize, joined(seed, salt))
	shuffled := make([]byte, len(payload))
	orderBytes := make([]byte, len(blockOrder)*4)
	for index, source := range blockOrder {
		copy(shuffled[index*aes.BlockSize:], payload[source*aes.BlockSize:(source+1)*aes.BlockSize])
		binary.LittleEndian.PutUint32(orderBytes[index*4:], uint32(source))
	}
	byteOrder := permutation(len(payload), joined(key, salt, []byte{byte(len(payload))}))
	for index, source := range byteOrder {
		payload[index] = shuffled[source]
	}
	checksum := sha256.Sum256(joined(orderBytes, payload))
	header := make([]byte, 19)
	header[0] = 1
	copy(header[1:17], salt)
	binary.LittleEndian.PutUint16(header[17:], uint16(len(blockOrder)))
	packet := joined(header, orderBytes, payload, checksum[:8])
	return base64.NewEncoding(requestAlphabet).WithPadding(base64.NoPadding).EncodeToString(packet), nil
}

// responseKey derives the static bootstrap response key from its 32-byte seed
// and two declared integers encoded as unsigned little-endian 64-bit values.
func responseKey() []byte {
	material := hexLiteral("38f6913e38cf38e7c90ea6f1fa31bf7501108effc57f6cd778a3b9dd68e3f51c")
	for _, value := range []uint64{9633601958, 3000935290} {
		integer := make([]byte, 8)
		binary.LittleEndian.PutUint64(integer, value)
		material = append(material, integer...)
	}
	digest := sha256.Sum256(material)
	return digest[:]
}

// decryptEnvelope validates AES-GCM's tag before accepting plaintext. Framing is
// salt[16] || nonce[12] || ciphertext || tag[16]. The first eight plaintext bytes
// are the protocol prefix; they do not establish media expiration.
func decryptEnvelope(encoded string, baseKey []byte) ([]byte, error) {
	if len(encoded) > 512*1024 {
		return nil, errors.New("encrypted envelope exceeds limit")
	}
	packet, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(packet) < 52 {
		return nil, errors.New("encrypted envelope malformed")
	}
	key := sha256.Sum256(joined(baseKey, packet[:16]))
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return nil, errors.New("invalid response cipher material")
	}
	mode, err := cipher.NewGCM(block)
	if err != nil {
		return nil, errors.New("invalid response cipher mode")
	}
	plaintext, err := mode.Open(nil, packet[16:28], packet[28:], nil)
	if err != nil || len(plaintext) < 8 || !utf8.Valid(plaintext[8:]) {
		return nil, errors.New("response authentication or framing rejected")
	}
	return plaintext[8:], nil
}

// rc4Bytes reproduces the recognized zero-mask RC4 stage with explicit native
// KSA/PRGA. This is upstream compatibility, not a cipher for new security uses.
func rc4Bytes(key, input []byte) []byte {
	var state [256]byte
	for index := range state {
		state[index] = byte(index)
	}
	var second byte
	for index := range state {
		second += state[index] + key[index%len(key)]
		state[index], state[second] = state[second], state[index]
	}
	var first byte
	second = 0
	result := make([]byte, len(input))
	for index, value := range input {
		first++
		second += state[first]
		state[first], state[second] = state[second], state[first]
		result[index] = value ^ state[byte(state[first]+state[second])]
	}
	return result
}

type transformStage struct {
	key    string
	prefix string
	ops    [10]string
}

var challengeStages = [...]transformStage{
	{
		key:    "JZltXDoh7Eags8L7e0hyBaX6HTZ4ErE1rydu4MTAhx0=",
		prefix: "2PcZ/vEJN24=",
		ops:    [10]string{"sub226", "sub76", "rol2", "sub200", "xor59", "sub76", "xor59", "sub200", "xor59", "sub226"},
	},
	{
		key:    "T1CiTh7ajlEieB/+USBMBpwFLQqBy7r+u0v+LEDGDrQ=",
		prefix: "iXAPz1fd9GQ=",
		ops:    [10]string{"sub216", "rol1", "rol7", "xor59", "sub216", "xor59", "sub226", "rol7", "rol1", "rol2"},
	},
	{
		key:    "oO0GK4QVPEEc7k2uGOPwQ9+uO+g0cWi2tupQfOZH5m4=",
		prefix: "f7prYx4j",
		ops:    [10]string{"sub218", "rol2", "xor59", "rol1", "rol2", "ror2", "xor59", "sub218", "rol1", "add230"},
	},
	{
		key:    "oa2189LasXo4boT0vDgZDE02/YMqD1qtaXZQzM7MStw=",
		prefix: "xDGTyVE=",
		ops:    [10]string{"rol2", "sub200", "ror2", "sub200", "rol1", "rol1", "sub226", "ror2", "rol1", "rol7"},
	},
	{
		key:    "svlpt4PNgksvqHbiResvgC9SO7RZzgmm+TK1uXVNkAw=",
		prefix: "p3uks0I=",
		ops:    [10]string{"sub200", "rol2", "sub226", "add230", "sub76", "add230", "sub200", "sub218", "rol2", "rol1"},
	},
}

func transformByte(value byte, operation string) byte {
	switch operation {
	case "sub76":
		return value - 76
	case "sub200":
		return value - 200
	case "sub216":
		return value - 216
	case "sub218":
		return value - 218
	case "sub226":
		return value - 226
	case "add230":
		return value + 230
	case "rol1":
		return bits.RotateLeft8(value, 1)
	case "rol2":
		return bits.RotateLeft8(value, 2)
	case "rol7":
		return bits.RotateLeft8(value, -1)
	case "ror2":
		return bits.RotateLeft8(value, -2)
	default:
		return value ^ 59
	}
}

// challengeToken reverses the challenge text, hex-encodes its UTF-8 bytes, then
// applies the five byte transforms and RC4 stages in the statically traced order.
func challengeToken(response []byte) (string, error) {
	if len(response) == 0 || len(response) > 4096 || !utf8.Valid(response) {
		return "", errors.New("challenge text malformed")
	}
	// Observed challenge responses are ASCII; reject rather than guessing JS
	// UTF-16 reversal semantics for a newly introduced Unicode protocol.
	for _, value := range response {
		if value > 127 || value < 32 {
			return "", errors.New("unrecognized challenge text")
		}
	}
	reversed := append([]byte(nil), response...)
	for index := 0; index < len(reversed)/2; index++ {
		other := len(reversed) - index - 1
		reversed[index], reversed[other] = reversed[other], reversed[index]
	}
	payload := []byte(hex.EncodeToString(reversed))
	streamKeys := [...]string{
		"wCrbGEDVWBGRVggKP8pODbU55I28p/AnwOqtkKsAt8Q=",
		"4yIb/l2zh9Qz62DrakKcfjlcgmldjWIIfBadCsL/blo=",
		"xj1hV8IXKy38jEOtXojtsDXi3+Sr2D2ve2GhbcHKvLI=",
		"1OqtIu7/lgRxDn1zaGDc8PtVF0cIJZNOsiNse4qxKqQ=",
		"yOwHRwrYyRB1fwQFiSRjzVFZpWatYUr8xBzZP0zdpO4=",
	}
	for index, stage := range challengeStages {
		key, _ := base64.StdEncoding.DecodeString(stage.key)
		prefix, _ := base64.StdEncoding.DecodeString(stage.prefix)
		transformed := make([]byte, 0, len(payload)+len(prefix))
		for offset, value := range payload {
			if offset < len(prefix) {
				transformed = append(transformed, prefix[offset])
			}
			transformed = append(transformed, transformByte(value^key[offset%32], stage.ops[offset%10]))
		}
		streamKey, _ := base64.StdEncoding.DecodeString(streamKeys[index])
		payload = rc4Bytes(streamKey, transformed)
	}
	return base64.RawURLEncoding.EncodeToString(payload), nil
}
