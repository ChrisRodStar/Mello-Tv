package vidsrc

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"strings"
	"unicode/utf8"

	"golang.org/x/crypto/chacha20"
)

// wasmReader reads a bounded binary structure, never instructions. Only the
// recognized section layout and ChaCha20 function bodies are accepted.
type wasmReader struct {
	remaining []byte
}

func (r *wasmReader) take(size uint32) ([]byte, error) {
	if uint64(size) > uint64(len(r.remaining)) {
		return nil, errors.New("truncated static module")
	}
	value := r.remaining[:size]
	r.remaining = r.remaining[size:]
	return value, nil
}

func (r *wasmReader) unsigned() (uint32, error) {
	var value uint32
	for index := 0; index < 5; index++ {
		part, err := r.take(1)
		if err != nil || (index == 4 && part[0] > 15) {
			return 0, errors.New("invalid static integer")
		}
		value |= uint32(part[0]&127) << (7 * index)
		if part[0]&128 == 0 {
			return value, nil
		}
	}
	return 0, errors.New("invalid static integer")
}

// dataOffset accepts only positive i32.const active-segment offsets; negative
// offsets, expression evaluation, passive segments and alternate memories are
// outside the traced layout. Padded encodings are not needed by this generator.
func (r *wasmReader) dataOffset() (uint32, error) {
	var value uint32
	for index := 0; index < 5; index++ {
		part, err := r.take(1)
		if err != nil || (index == 4 && part[0] > 7) {
			return 0, errors.New("invalid static offset")
		}
		value |= uint32(part[0]&127) << (7 * index)
		if part[0]&128 == 0 {
			if part[0]&64 != 0 {
				return 0, errors.New("negative static offset")
			}
			return value, nil
		}
	}
	return 0, errors.New("invalid static offset")
}

func hasDigest(value []byte, digest string) bool {
	actual := sha256.Sum256(value)
	return hex.EncodeToString(actual[:]) == digest
}

// moduleKey recognizes only the traced native ChaCha20 implementation. Unused
// decoy functions, globals and junk data rotate with the five-minute window.
// The final three functions (20-round block, allocator, nonce/counter/XOR wrapper),
// types and memory are pinned after normalizing traced mask addresses and call
// indices. No provider code is compiled or run.
func moduleKey(module []byte) ([]byte, error) {
	if len(module) > 128*1024 || len(module) < 8 || !bytes.Equal(module[:8], []byte{0, 97, 115, 109, 1, 0, 0, 0}) {
		return nil, errors.New("unrecognized static module")
	}
	reader := wasmReader{remaining: module[8:]}
	sections := make(map[byte][]byte)
	lastSection := byte(0)
	for len(reader.remaining) > 0 {
		identifier, err := reader.take(1)
		if err != nil {
			return nil, err
		}
		size, err := reader.unsigned()
		if err != nil {
			return nil, err
		}
		payload, err := reader.take(size)
		if err != nil {
			return nil, err
		}
		sectionID := identifier[0]
		if sectionID <= lastSection || (sectionID != 1 && sectionID != 3 && sectionID != 5 && sectionID != 6 && sectionID != 7 && sectionID != 10 && sectionID != 11) {
			return nil, errors.New("unrecognized static sections")
		}
		lastSection = sectionID
		sections[sectionID] = payload
	}

	for sectionID, digest := range map[byte]string{
		1: "4ea697dca6034d419831565f2ead73494f4c732e97c7632d5df08e146879076f",
		5: "dbecab43cb8facaa2f8537a707d982cbc168a7767f3b0d3b1f3cd4aa5ef272bf",
	} {
		if !hasDigest(sections[sectionID], digest) {
			return nil, errors.New("unrecognized static layout")
		}
	}

	decoyCount, err := moduleFunctions(sections[3])
	if err != nil {
		return nil, err
	}
	if err := checkExports(sections[7], decoyCount); err != nil {
		return nil, err
	}
	var maskOffset uint32
	code := wasmReader{remaining: sections[10]}
	count, err := code.unsigned()
	if err != nil || count != decoyCount+3 {
		return nil, errors.New("unrecognized static functions")
	}
	for index := uint32(0); index < count; index++ {
		size, err := code.unsigned()
		if err != nil {
			return nil, err
		}
		body, err := code.take(size)
		if err != nil {
			return nil, err
		}
		digests := []string{
			"a6058c9901cb96927c0da12657be846dff74134fb4e198154f66fbbeb5ede21a",
			"4c292b28a90f8d74b9967e9ab678f12308c6627c3a2b859fbf1bc43208e246ff",
			"3a5e8ab52d551a477c02c6bb906c7b8a109fab114feb0019317671da55718dbc",
		}
		if index == decoyCount {
			var err error
			body, maskOffset, err = normalizeBlock(body)
			if err != nil {
				return nil, err
			}
		}
		if index == decoyCount+2 {
			body, err = normalizeDecrypt(body, decoyCount)
			if err != nil {
				return nil, err
			}
		}
		if index >= decoyCount && !hasDigest(body, digests[index-decoyCount]) {
			return nil, errors.New("unrecognized static cipher")
		}
	}
	if len(code.remaining) != 0 {
		return nil, errors.New("trailing static code")
	}

	data := wasmReader{remaining: sections[11]}
	count, err = data.unsigned()
	if err != nil || count > 32 {
		return nil, errors.New("unrecognized static data")
	}
	memory := make([]byte, 4096)
	occupied := make([]bool, len(memory))
	for index := uint32(0); index < count; index++ {
		mode, err := data.unsigned()
		if err != nil || mode != 0 {
			return nil, errors.New("unrecognized static data mode")
		}
		opcode, err := data.take(1)
		if err != nil || opcode[0] != 0x41 {
			return nil, errors.New("unrecognized static initializer")
		}
		offset, err := data.dataOffset()
		if err != nil {
			return nil, err
		}
		end, err := data.take(1)
		if err != nil || end[0] != 0x0b {
			return nil, errors.New("unrecognized static expression")
		}
		size, err := data.unsigned()
		if err != nil || uint64(offset)+uint64(size) > uint64(len(memory)) {
			return nil, errors.New("static data outside recognized memory")
		}
		value, err := data.take(size)
		if err != nil {
			return nil, err
		}
		for position := offset; position < offset+size; position++ {
			if occupied[position] {
				return nil, errors.New("overlapping static data")
			}
			occupied[position] = true
		}
		copy(memory[offset:], value)
	}
	if len(data.remaining) != 0 {
		return nil, errors.New("trailing static data")
	}
	key := make([]byte, 32)
	for index := range key {
		if !occupied[index] || !occupied[int(maskOffset)+index] {
			return nil, errors.New("static key data absent")
		}
		key[index] = memory[index] ^ memory[int(maskOffset)+index]
	}
	return key, nil
}

// decryptStreams performs IETF ChaCha20 (20 rounds, no KDF or MAC), with the
// first 12 decoded bytes as nonce and initial block counter zero. The window key
// is the bytewise XOR of data at 0 and the statically declared rotating mask
// offset, not a timestamp-derived key.
// This upstream envelope is unauthenticated; validate plaintext and destinations.
func decryptStreams(encoded string, module []byte) ([]string, error) {
	key, err := moduleKey(module)
	if err != nil {
		return nil, err
	}
	return decryptStreamsWithKey(encoded, key)
}

// The key is immutable and comes only from successful static module recognition.
func decryptStreamsWithKey(encoded string, key []byte) ([]string, error) {
	ciphertext, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(ciphertext) <= 12 || len(ciphertext) > 64*1024 {
		return nil, errors.New("invalid encrypted stream list")
	}
	cipher, err := chacha20.NewUnauthenticatedCipher(key, ciphertext[:12])
	if err != nil {
		return nil, err
	}
	plaintext := make([]byte, len(ciphertext)-12)
	cipher.XORKeyStream(plaintext, ciphertext[12:])
	if !utf8.Valid(plaintext) || bytes.ContainsAny(plaintext, "\x00\r") {
		return nil, errors.New("invalid decrypted stream list")
	}
	streams := strings.Split(string(plaintext), "\n")
	result := make([]string, 0, len(streams))
	for _, stream := range streams {
		if stream != "" {
			result = append(result, stream)
		}
	}
	return result, nil
}

// normalizeBlock accepts only the sixteen key-mask address immediates changing
// within the exact traced block. Each word occurs in initialization and feed-
// forward. Canonicalizing those constants permits a full-body fingerprint check;
// any other opcode, round, load, or call change is rejected. This is static
// template matching, not a WASM evaluator or general instruction parser.
func normalizeBlock(body []byte) ([]byte, uint32, error) {
	normalized := append([]byte(nil), body...)
	var maskOffset uint32
	for word := uint32(0); word < 8; word++ {
		prefix := []byte{0x41, byte(word * 4), 0x28, 2, 0, 0x41}
		positions := 0
		cursor := 0
		for cursor < len(body) {
			position := bytes.Index(body[cursor:], prefix)
			if position < 0 {
				break
			}
			position += cursor + len(prefix)
			reader := wasmReader{remaining: body[position:]}
			offset, err := reader.dataOffset()
			consumed := len(body[position:]) - len(reader.remaining)
			if err != nil || consumed != 2 || len(reader.remaining) < 4 || !bytes.Equal(reader.remaining[:4], []byte{0x28, 2, 0, 0x73}) || offset < word*4 {
				return nil, 0, errors.New("unrecognized static key load")
			}
			base := offset - word*4
			if base < 512 || base > 3968 || base%128 != 0 || (maskOffset != 0 && maskOffset != base) {
				return nil, 0, errors.New("inconsistent static key load")
			}
			maskOffset = base
			canonical := uint32(3584) + word*4
			normalized[position] = byte(canonical&127) | 128
			normalized[position+1] = byte(canonical >> 7)
			positions++
			cursor = position + consumed
		}
		if positions != 2 {
			return nil, 0, errors.New("static key loads absent or ambiguous")
		}
	}
	return normalized, maskOffset, nil
}

func checkExports(section []byte, decoyCount uint32) error {
	reader := wasmReader{remaining: section}
	count, err := reader.unsigned()
	if err != nil || count != decoyCount+3 {
		return errors.New("unrecognized static exports")
	}
	names := make(map[string]bool)
	indices := make(map[uint32]bool)
	for index := uint32(0); index < count; index++ {
		size, err := reader.unsigned()
		if err != nil || size > 64 {
			return errors.New("invalid static export name")
		}
		name, err := reader.take(size)
		if err != nil {
			return err
		}
		kind, err := reader.take(1)
		if err != nil {
			return err
		}
		target, err := reader.unsigned()
		if err != nil || names[string(name)] {
			return errors.New("invalid static export")
		}
		names[string(name)] = true
		if string(name) == "memory" && kind[0] == 2 && target == 0 {
			continue
		}
		if kind[0] != 0 || indices[target] || target == decoyCount || target > decoyCount+2 ||
			(target == decoyCount+1 && string(name) != "alloc") || (target == decoyCount+2 && string(name) != "decrypt") ||
			(target < decoyCount && (string(name) == "alloc" || string(name) == "decrypt" || string(name) == "memory")) {
			return errors.New("unrecognized static export mapping")
		}
		indices[target] = true
	}
	if len(reader.remaining) != 0 || !names["memory"] || !names["alloc"] || !names["decrypt"] || len(indices) != int(decoyCount)+2 {
		return errors.New("required static exports absent")
	}
	return nil
}

// The generator varies the count of unused unary decoy functions. Only the
// final block/allocator/decrypt signatures are meaningful; imports/start/table
// sections remain prohibited and none of these functions is executed.
func moduleFunctions(section []byte) (uint32, error) {
	reader := wasmReader{remaining: section}
	count, err := reader.unsigned()
	if err != nil || count < 3 || count > 19 {
		return 0, errors.New("unrecognized static function layout")
	}
	decoyCount := count - 3
	for index := uint32(0); index < count; index++ {
		signature, err := reader.unsigned()
		expected := uint32(0)
		if index == decoyCount {
			expected = 1
		}
		if index == decoyCount+2 {
			expected = 2
		}
		if err != nil || signature != expected {
			return 0, errors.New("unrecognized static signature")
		}
	}
	if len(reader.remaining) != 0 {
		return 0, errors.New("trailing static signatures")
	}
	return decoyCount, nil
}

func normalizeDecrypt(body []byte, blockIndex uint32) ([]byte, error) {
	// The traced wrapper has exactly one call, following counter and block-output
	// arguments. Check its immediate index, normalize it, then fingerprint the
	// entire wrapper. This cannot accept an extra call or another transformation.
	prefix := []byte{0x20, 4, 0x41, 0xc0, 0, 0x10}
	position := bytes.Index(body, prefix)
	if position < 0 || position+len(prefix) >= len(body) || bytes.Count(body, prefix) != 1 || body[position+len(prefix)] != byte(blockIndex) {
		return nil, errors.New("unrecognized static block call")
	}
	normalized := append([]byte(nil), body...)
	normalized[position+len(prefix)] = 3
	return normalized, nil
}
