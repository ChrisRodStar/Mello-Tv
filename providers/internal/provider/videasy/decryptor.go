package videasy

import (
	"encoding/base64"
	"errors"
	"strconv"
	"strings"
)

var (
	magic     = []byte{109, 118, 109, 49} // "mvm1"
	hashTable = [16]uint32{
		1116352408, 1899447441, 3049323471, 3921009573,
		961987163, 1508970993, 2453635748, 2870763221,
		3624381080, 310598401, 607225278, 1426881987,
		1925078388, 2162078206, 2614888103, 3248222580,
	}
)

func rotl32(x uint32, n uint32) uint32 {
	n &= 31
	if n == 0 {
		return x
	}
	return (x << n) | (x >> (32 - n))
}

func hash32(x uint32) uint32 {
	x ^= x >> 16
	x = x * 2246822507
	x ^= x >> 13
	x = x * 3266489909
	x ^= x >> 16
	return x
}

func fnv1a(s string) uint32 {
	h := uint32(2166136261)
	for i := 0; i < len(s); i++ {
		h = (h ^ uint32(s[i])) * 16777619
	}
	return hash32(h)
}

type streamState struct {
	S     [61]uint32
	isSet [61]bool
	acc   uint32
}

func initStream(seed string, secondKey uint32) *streamState {
	st := &streamState{}
	a := hash32(fnv1a(seed) ^ hash32(secondKey^2654435769))

	for i := uint32(0); i < 8; i++ {
		if (i*(i+1))&1 == 0 {
			idx := a % 61
			a = rotl32(a+2654435769, 7+(7&i))
			st.S[idx] = a ^ hash32(a)
			st.isSet[idx] = true
			a = hash32(a + idx)
		} else {
			st.S[i] = hashTable[15&i]
			st.isSet[i] = true
		}
	}
	st.acc = hash32(2779096485 ^ a)
	return st
}

func (st *streamState) nextByte(ctr uint32) uint32 {
	n := st.acc % 61
	inSet := uint32(0)
	if st.isSet[n] {
		inSet = 0xFFFFFFFF
	}
	d := st.S[n]
	x := d ^ (2654435769 * (ctr + 1))
	y := (st.acc ^ x) | (st.acc & x & inSet)
	part1 := rotl32(y+st.acc, 31&n)
	part2 := rotl32(st.acc, 31&(n*7))
	no := hash32((part1 ^ part2) + 2654435769)
	st.S[n] = no
	st.isSet[n] = true
	st.acc = no
	return no
}

func generateKeystream(seed string, secondKey uint32, length int) []byte {
	st := initStream(seed, secondKey)
	out := make([]byte, length)
	ctr := uint32(0)

	for i := 0; i < length; {
		b := st.nextByte(ctr)
		ctr++
		out[i] = byte(b & 0xFF)
		i++
		if i < length {
			out[i] = byte((b >> 8) & 0xFF)
			i++
		}
		if i < length {
			out[i] = byte((b >> 16) & 0xFF)
			i++
		}
		if i < length {
			out[i] = byte((b >> 24) & 0xFF)
			i++
		}
	}
	return out
}

// DecryptPayload decrypts a base64-encoded Videasy response blob using the stream cipher.
func DecryptPayload(payload, seed string, tmdbID int64) ([]byte, error) {
	if len(payload) < 20 {
		return nil, errors.New("payload too short")
	}

	// Normalize URL-safe base64 padding
	b64 := strings.ReplaceAll(strings.ReplaceAll(payload, "-", "+"), "_", "/")
	for len(b64)%4 != 0 {
		b64 += "="
	}

	data, err := base64.StdEncoding.DecodeString(b64)
	if err != nil {
		return nil, err
	}

	secondKeyStr := strconv.FormatInt(tmdbID, 10)
	secondKeyVal, _ := strconv.ParseUint(secondKeyStr, 10, 32)
	ks := generateKeystream(seed, uint32(secondKeyVal), len(data))

	for i := 0; i < len(data); i++ {
		data[i] ^= ks[i]
	}

	if len(data) < len(magic) {
		return nil, errors.New("decrypted data too short")
	}

	for i := 0; i < len(magic); i++ {
		if data[i] != magic[i] {
			return nil, errors.New("invalid encrypted payload magic")
		}
	}

	return data[len(magic):], nil
}
