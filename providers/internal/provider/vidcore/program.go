package vidcore

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"stv-providers/internal/model"
	"fmt"
	"regexp"
	"strconv"
	"strings"
)

// The following SHA-256 digests lock the native implementation to known, audited
// player bytecode versions. If upstream deploys modified obfuscation or instructions,
// compilation fails fast with model.ProtocolChange rather than executing unverified code.
const (
	materialDigest = "af13d5c75ee77178a31554392c906bd94ee3e979d62691690108f3ded337a885"
	listDigest     = "a39fcc5958024e588c963ebd34ece56b49f59e37539edab7df082288b28a23e3"
	unlockDigest   = "ac9a1f7e7b5b4d2b28a32c2e73f6dc494f2596ed8dfedac3e6ed86b5f6545086"

	legacyMaterialDigest = "52dbf762faf5d6f5fee0aa71e8cffda15567a7640fd217610eb8a13fda8ff8d5"
	legacyListDigest     = "e5872b95b115bc1ab52e148454837d0354cbd96c73c8fbf28fa75c6808c749d1"
	legacyUnlockDigest   = "18948e99225f9d2fd88a05e0615a64b5d34e80ed3a9597a75da6beb560e560e4"
)

// byteStage represents a single transformation round in the ticket generation pipeline.
type byteStage struct {
	Key        []byte
	Prefix     []byte
	Operations [10]byte
}

// protocol encapsulates cryptographic constants, actions, and mappings extracted from the player bytecode.
type protocol struct {
	Base            string
	ChallengeAction string
	ListAction      string
	UnlockAction    string
	Key             []byte
	IV              []byte
	Seed            []byte
	ResponseSeed    []byte
	ResponseNumbers []uint64
	Alphabet        map[byte]byte
	Stages          [5]byteStage
	RC4Keys         [5][]byte
}

// protocolError builds a standard protocol_change Failure error.
func protocolError(detail string) error {
	return model.Fail(model.ProtocolChange, "native_protocol", detail)
}

var (
	codecDeclaration      = regexp.MustCompile(`([\w$]+)=\[([\d,]+)\]\.map\(function\([\w$]+\)\{return String\.fromCharCode\((\d+)\^[\w$]+\)\}\)\.join\(""\)`)
	shiftCodecDeclaration = regexp.MustCompile(`([\w$]+)="([^"]+)"\.split\(""\)\.map\(function\([\w$]+\)\{return String\.fromCharCode\([\w$]+\.charCodeAt\(0\)-(\d+)\)\}\)\.join\(""\)`)
	byteArrayDeclaration  = regexp.MustCompile(`([\w$]+)=new Uint8Array\(\[([\d,]+)\]\)`)
	blobDeclaration       = regexp.MustCompile(`([\w$]+)=[\w$]+\.from\("([^"]+)",([\w$]+)\)`)
	xorBlobDeclaration    = regexp.MustCompile(`([\w$]+)=function\([^)]*\)\{[^}]*\.from\(e,([\w$]+)\)[^}]*\^=(\d+);return [\w$]+\}\("([^"]+)",0\)`)
	orderDeclaration      = regexp.MustCompile(`case \d+:[\w$]+\[(\d+)\]=([\w$]+)`)
	maskDeclaration       = regexp.MustCompile(`a=(0x[\da-f]+),r=0;r<256;r\+\+\)\{var s=a=a\+(0x[\da-f]+)>>>0;`)
)

// decodeLiteral decodes a base64 or hex encoded string literal from static player source.
func decodeLiteral(raw, codec string) ([]byte, error) {
	switch codec {
	case "base64":
		return base64.StdEncoding.DecodeString(raw)
	case "hex":
		// Truncate to even length to guarantee clean hex pairs.
		evenLength := (len(raw) / 2) * 2
		return hex.DecodeString(raw[:evenLength])
	default:
		return nil, protocolError("unsupported literal encoding: " + codec)
	}
}

// byteMask generates a 256-byte unmasking array using MurmurHash3 finalizer constants.
func byteMask(seed, step uint32) [256]byte {
	var out [256]byte
	for i := range out {
		seed += step
		x := seed
		x = (x ^ (x >> 16)) * 0x85ebca6b
		x = (x ^ (x >> 13)) * 0xc2b2ae35
		x ^= x >> 16
		out[i] = byte(x)
	}
	return out
}

// decodeProgram parses static AST fragments, reorders bytecode chunks, and applies unmasking.
func decodeProgram(prefix, stage string, expectedParts int) ([]byte, error) {
	codecs := make(map[string]string)
	blobs := make(map[string][]byte)

	// Decode string codec maps (e.g. "base64", "hex").
	for _, match := range codecDeclaration.FindAllStringSubmatch(prefix, -1) {
		xorKey, err := strconv.Atoi(match[3])
		if err != nil {
			return nil, err
		}

		var builder strings.Builder
		for _, rawVal := range strings.Split(match[2], ",") {
			codePoint, err := strconv.Atoi(rawVal)
			if err != nil {
				return nil, err
			}
			builder.WriteRune(rune(codePoint ^ xorKey))
		}
		codecs[match[1]] = builder.String()
	}

	for _, match := range shiftCodecDeclaration.FindAllStringSubmatch(prefix, -1) {
		shift, err := strconv.Atoi(match[3])
		if err != nil {
			return nil, err
		}
		var builder strings.Builder
		for _, r := range match[2] {
			builder.WriteRune(rune(int(r) - shift))
		}
		codecs[match[1]] = builder.String()
	}

	// Decode static Uint8Array byte literals.
	for _, match := range byteArrayDeclaration.FindAllStringSubmatch(prefix, -1) {
		values := strings.Split(match[2], ",")
		data := make([]byte, len(values))
		for i, rawVal := range values {
			byteVal, err := strconv.ParseUint(rawVal, 10, 8)
			if err != nil {
				return nil, err
			}
			data[i] = byte(byteVal)
		}
		blobs[match[1]] = data
	}

	// Decode Buffer.from string blobs.
	for _, match := range blobDeclaration.FindAllStringSubmatch(prefix, -1) {
		if codec, exists := codecs[match[3]]; exists {
			data, err := decodeLiteral(match[2], codec)
			if err != nil {
				return nil, err
			}
			blobs[match[1]] = data
		}
	}

	// Decode XOR-modified string blobs.
	for _, match := range xorBlobDeclaration.FindAllStringSubmatch(prefix, -1) {
		data, err := decodeLiteral(match[4], codecs[match[2]])
		if err != nil {
			return nil, err
		}

		xorVal, err := strconv.ParseUint(match[3], 10, 8)
		if err != nil {
			return nil, err
		}

		for i := range data {
			data[i] ^= byte(xorVal)
		}
		blobs[match[1]] = data
	}

	// Assemble bytecode parts according to switch case order.
	orderedParts := make([][]byte, expectedParts)
	for _, match := range orderDeclaration.FindAllStringSubmatch(stage, -1) {
		partIndex, err := strconv.Atoi(match[1])
		if err != nil || partIndex >= expectedParts {
			return nil, protocolError("unexpected bytecode part ordering")
		}
		if orderedParts[partIndex] != nil {
			return nil, protocolError("duplicate bytecode part index")
		}
		orderedParts[partIndex] = blobs[match[2]]
	}

	var concatenated []byte
	for _, part := range orderedParts {
		if len(part) == 0 {
			return nil, protocolError("missing bytecode data partition")
		}
		concatenated = append(concatenated, part...)
	}

	// Parse PRNG mask constants and unmask concatenated bytecode.
	maskMatches := maskDeclaration.FindAllStringSubmatch(stage, -1)
	if len(maskMatches) != 2 {
		return nil, protocolError("bytecode mask structure changed")
	}

	var keys [2][256]byte
	for i, match := range maskMatches {
		seed, err := strconv.ParseUint(match[1][2:], 16, 32)
		if err != nil {
			return nil, err
		}
		step, err := strconv.ParseUint(match[2][2:], 16, 32)
		if err != nil {
			return nil, err
		}
		keys[i] = byteMask(uint32(seed), uint32(step))
	}

	for i := range concatenated {
		concatenated[i] ^= keys[0][i%256] ^ keys[1][i%256]
	}

	return concatenated, nil
}

// stringConstant extracts a length-prefixed string constant from verified bytecode at a known offset.
func stringConstant(code []byte, offset int) (string, error) {
	if offset < 0 || offset+4 > len(code) {
		return "", protocolError("mapped constant absent")
	}
	if (code[offset] != 103 || code[offset+1] != 188) && (code[offset] != 89 || code[offset+1] != 4) {
		return "", protocolError("mapped constant absent")
	}

	length := int(binary.BigEndian.Uint16(code[offset+2 : offset+4]))
	if offset+4+length > len(code) {
		return "", protocolError("truncated mapped constant")
	}

	return string(code[offset+4 : offset+4+length]), nil
}

// compileProtocol statically extracts and validates cryptographic parameters from JS source.
func compileProtocol(source string) (*protocol, error) {
	material, err := ParseMaterial(source)
	if err != nil {
		return nil, err
	}

	moduleIndices := modulePattern.FindStringIndex(source)
	boundaryIndices := boundaryPattern.FindStringIndex(source)
	prefix := source[moduleIndices[0]:boundaryIndices[0]]

	listStartMatches := listPattern.FindStringIndex(source[boundaryIndices[0]:])
	listStart := boundaryIndices[0] + listStartMatches[0]
	listEnd := listStart + strings.Index(source[listStart:], ",()=>{clearInterval(")

	tail := source[listEnd:]
	unlockStartMatches := unlockPattern.FindStringIndex(tail)
	unlockCode := tail[unlockStartMatches[0]:]
	unlockEndMatches := unlockEndPattern.FindStringIndex(unlockCode)
	unlockCode = unlockCode[:unlockEndMatches[0]]

	// Validate raw material fingerprint.
	materialFingerprint := fmt.Sprintf("%x", sha256.Sum256([]byte(prefix+"\n"+source[listStart:listEnd]+"\n"+unlockCode)))
	if materialFingerprint != materialDigest && materialFingerprint != legacyMaterialDigest {
		return nil, protocolError("unmapped player material version")
	}

	// Extract and unmask list and unlock bytecode.
	listBytecode, err := decodeProgram(prefix, source[listStart:listEnd], 6)
	if err != nil {
		return nil, protocolError("list bytecode extraction failed")
	}

	unlockBytecode, err := decodeProgram(prefix, unlockCode, 5)
	if err != nil {
		return nil, protocolError("unlock bytecode extraction failed")
	}

	// Validate compiled bytecode digests.
	listFingerprint := fmt.Sprintf("%x", sha256.Sum256(listBytecode))
	unlockFingerprint := fmt.Sprintf("%x", sha256.Sum256(unlockBytecode))
	if (listFingerprint != listDigest || unlockFingerprint != unlockDigest) &&
		(listFingerprint != legacyListDigest || unlockFingerprint != legacyUnlockDigest) {
		return nil, protocolError("unmapped player instruction version")
	}

	proto := &protocol{
		Base:         material.Base,
		UnlockAction: material.Action,
		Alphabet:     make(map[byte]byte),
	}

	readConstant := func(offset int) string {
		val, _ := stringConstant(listBytecode, offset)
		return val
	}

	var stageKeyOffsets, prefixOffsets, rc4Offsets []int

	if materialFingerprint == materialDigest {
		if readConstant(45623) != material.Base || readConstant(51230) != material.Base {
			return nil, protocolError("catalog/unlock base mismatch")
		}

		proto.ChallengeAction = readConstant(45706)
		proto.ListAction = readConstant(51313)
		proto.Key, _ = decodeLiteral(readConstant(36912), "hex")
		proto.IV, _ = decodeLiteral(readConstant(37038), "hex")
		proto.Seed, _ = decodeLiteral(readConstant(37132), "hex")
		proto.ResponseSeed, _ = decodeLiteral(readConstant(47894), "hex")

		for _, offset := range []int{48303, 48405} {
			num, err := strconv.ParseUint(readConstant(offset), 10, 64)
			if err != nil {
				return nil, protocolError("invalid response-key component")
			}
			proto.ResponseNumbers = append(proto.ResponseNumbers, num)
		}

		stageKeyOffsets = []int{29963, 31265, 32632, 34070, 35495}
		prefixOffsets = []int{30046, 31348, 32715, 34153, 35578}
		rc4Offsets = []int{46598, 47114, 47372, 47629, 46856}
	} else {
		if readConstant(45636) != material.Base || readConstant(51272) != material.Base {
			return nil, protocolError("catalog/unlock base mismatch")
		}

		proto.ChallengeAction = readConstant(45754)
		proto.ListAction = readConstant(51390)
		proto.Key, _ = decodeLiteral(readConstant(36941), "hex")
		proto.IV, _ = decodeLiteral(readConstant(37067), "hex")
		proto.Seed, _ = decodeLiteral(readConstant(37161), "hex")
		proto.ResponseSeed, _ = decodeLiteral(readConstant(47943), "hex")

		for _, offset := range []int{48351, 48453} {
			num, err := strconv.ParseUint(readConstant(offset), 10, 64)
			if err != nil {
				return nil, protocolError("invalid response-key component")
			}
			proto.ResponseNumbers = append(proto.ResponseNumbers, num)
		}

		stageKeyOffsets = []int{29980, 31282, 32650, 34087, 35525}
		prefixOffsets = []int{30063, 31365, 32733, 34170, 35608}
		rc4Offsets = []int{46646, 47162, 47420, 47678, 46904}
	}

	// Parse URL alphabet substitution mapping.
	alphabetMatches := regexp.MustCompile(`let [\w$]+=(\[[^;]+?\]),[\w$]+=JSON.parse\('(\[[^']+\])'\);`).FindStringSubmatch(prefix)
	if len(alphabetMatches) != 3 {
		return nil, protocolError("URL alphabet missing")
	}

	var sourceChars, targetChars []string
	if json.Unmarshal([]byte(alphabetMatches[1]), &sourceChars) != nil ||
		json.Unmarshal([]byte(alphabetMatches[2]), &targetChars) != nil ||
		len(sourceChars) != 64 || len(targetChars) != 64 {
		return nil, protocolError("invalid URL alphabet structure")
	}

	seenTargets := make(map[byte]bool)
	for i, srcChar := range sourceChars {
		if len(srcChar) != 1 || len(targetChars[i]) != 1 || seenTargets[targetChars[i][0]] {
			return nil, protocolError("invalid alphabet permutation")
		}
		proto.Alphabet[srcChar[0]] = targetChars[i][0]
		seenTargets[targetChars[i][0]] = true
	}

	// 5-stage transformation pipeline operations.
	stageOperations := [5][10]byte{
		{0, 1, 2, 3, 4, 1, 4, 3, 4, 0},
		{5, 2, 4, 6, 2, 7, 4, 5, 6, 8},
		{2, 3, 7, 3, 6, 6, 0, 7, 6, 9},
		{3, 2, 0, 8, 1, 8, 3, 5, 2, 6},
		{10, 6, 9, 4, 10, 4, 0, 9, 6, 2},
	}

	for i, offset := range stageKeyOffsets {
		proto.Stages[i].Key, _ = decodeLiteral(readConstant(offset), "base64")
		proto.Stages[i].Operations = stageOperations[i]
	}

	for i, offset := range prefixOffsets {
		proto.Stages[i].Prefix, _ = decodeLiteral(readConstant(offset), "base64")
	}

	for i, offset := range rc4Offsets {
		proto.RC4Keys[i], _ = decodeLiteral(readConstant(offset), "base64")
	}

	// Verify cryptographic key dimensions.
	if len(proto.Key) != 32 || len(proto.IV) != 16 || len(proto.ResponseSeed) != 32 || len(proto.Seed) == 0 {
		return nil, protocolError("invalid protocol key dimensions")
	}

	for i := range proto.Stages {
		if len(proto.Stages[i].Key) != 32 || len(proto.RC4Keys[i]) != 32 {
			return nil, protocolError("invalid stage key dimensions")
		}
	}

	return proto, nil
}
