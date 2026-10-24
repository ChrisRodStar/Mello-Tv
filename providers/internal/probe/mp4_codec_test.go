package probe

import (
	"context"
	"encoding/binary"
	"slices"
	"testing"

	"stv-providers/internal/model"
	"stv-providers/internal/selection"
)

func fixtureDescriptor(tag byte, payload []byte) []byte {
	return append([]byte{tag, byte(len(payload))}, payload...)
}

func fixtureAudioConfiguration(specific []byte, flags byte, optional []byte) []byte {
	decoder := make([]byte, 13)
	decoder[0] = 0x40
	decoder[1] = 5<<2 | 1
	decoder = append(decoder, fixtureDescriptor(5, specific)...)
	elementaryStream := append([]byte{0, 1, flags}, optional...)
	elementaryStream = append(elementaryStream, fixtureDescriptor(4, decoder)...)
	elementaryStream = append(elementaryStream, fixtureDescriptor(6, []byte{2})...)
	return append(make([]byte, 4), fixtureDescriptor(3, elementaryStream)...)
}

func TestMP4DetailedCodecsParticipateInSharedSelection(t *testing.T) {
	videoEntry := make([]byte, 78)
	binary.BigEndian.PutUint16(videoEntry[24:], 1920)
	binary.BigEndian.PutUint16(videoEntry[26:], 1080)
	videoEntry = append(videoEntry, fixtureBox("avcC", []byte{1, 0x64, 0, 0x28, 0xff, 0xe0, 0})...)
	audioEntry := append(make([]byte, 28), fixtureBox("esds", fixtureAudioConfiguration([]byte{0x12, 0x10}, 0, nil))...)
	var tracks []byte
	for _, entry := range [][]byte{fixtureBox("avc1", videoEntry), fixtureBox("mp4a", audioEntry)} {
		description := make([]byte, 8)
		binary.BigEndian.PutUint32(description[4:], 1)
		description = append(description, entry...)
		track := fixtureBox("trak", fixtureBox("mdia", fixtureBox("minf", fixtureBox("stbl", fixtureBox("stsd", description)))))
		tracks = append(tracks, track...)
	}
	file := append(fixtureBox("ftyp", []byte("isom\x00\x00\x00\x00isom")), fixtureBox("moov", tracks)...)
	file = append(file, fixtureBox("mdat", make([]byte, 512*1024))...)
	client := &mp4FixtureClient{
		body: file,
	}
	pool := &Pool{
		client: client,
		limits: DefaultLimits(),
	}
	title := model.Title{
		Type:   model.Movie,
		TMDBID: 27205,
	}
	candidate := model.Candidate{
		Title:  title,
		URL:    "https://example.com/file.mp4",
		Format: model.MP4,
	}
	result := pool.probeMP4(context.Background(), candidate, &byteBudget{remaining: 1024 * 1024}, &byteBudget{remaining: 1024 * 1024})
	if !result.Observation.Accessible || result.Observation.Variant == nil ||
		!slices.Equal(result.Observation.Variant.Codecs, []string{"avc1.640028", "mp4a.40.2"}) {
		t.Fatalf("detailed codec metadata lost: %+v", result.Observation)
	}
	lookup := model.Lookup{
		Title: title,
		Device: model.Device{
			Formats: []model.Format{model.MP4},
			Codecs:  []string{"avc1.640028", "mp4a.40.2"},
		},
	}
	if eligible, reason := selection.Eligible(lookup, result); !eligible {
		t.Fatalf("compatible detailed profile rejected: %s", reason)
	}
	lookup.Device.Codecs = []string{"avc1", "mp4a"}
	if eligible, reason := selection.Eligible(lookup, result); !eligible {
		t.Fatalf("compatible codec families rejected: %s", reason)
	}
	lookup.Device.Codecs = []string{"avc1.42001e", "mp4a.40.2"}
	if eligible, _ := selection.Eligible(lookup, result); eligible {
		t.Fatal("different AVC profile accepted")
	}
	lookup.Device.Codecs = []string{"avc1.640028", "mp4a.40.5"}
	if eligible, _ := selection.Eligible(lookup, result); eligible {
		t.Fatal("different audio object type accepted")
	}
}

func TestMP4CodecConfigurationBoundsAndObjectTypes(t *testing.T) {
	for _, testCase := range []struct {
		Name          string
		Kind          string
		Configuration []byte
		Expected      string
	}{
		{
			Name:          "avc3-profile",
			Kind:          "avc3",
			Configuration: fixtureBox("avcC", []byte{1, 0x42, 0xe0, 0x1e, 0xff, 0xe0, 0}),
			Expected:      "avc3.42e01e",
		},
		{
			Name:          "optional-ES-fields",
			Kind:          "mp4a",
			Configuration: fixtureBox("esds", fixtureAudioConfiguration([]byte{0x2a, 0x10}, 0xe0, []byte{0, 2, 3, 'u', 'r', 'l', 0, 3})),
			Expected:      "mp4a.40.5",
		},
		{
			Name:          "escaped-audio-type",
			Kind:          "mp4a",
			Configuration: fixtureBox("esds", fixtureAudioConfiguration([]byte{0xf8, 0x40}, 0, nil)),
			Expected:      "mp4a.40.34",
		},
		{
			Name:     "missing-config-retains-family",
			Kind:     "avc1",
			Expected: "avc1",
		},
	} {
		t.Run(testCase.Name, func(t *testing.T) {
			actual, err := mp4SampleCodec(testCase.Kind, testCase.Configuration)
			if err != nil || actual != testCase.Expected {
				t.Fatalf("codec = %q, error = %v", actual, err)
			}
		})
	}

	validAVC := fixtureBox("avcC", []byte{1, 0x64, 0, 0x28, 0xff, 0xe0, 0})
	for _, configuration := range [][]byte{
		fixtureBox("avcC", []byte{1, 0x64}),
		fixtureBox("avcC", []byte{2, 0x64, 0, 0x28, 0xff, 0xe0, 0}),
		append(append([]byte(nil), validAVC...), validAVC...),
	} {
		if _, err := mp4SampleCodec("avc1", configuration); err == nil {
			t.Fatal("invalid AVC configuration accepted")
		}
	}
	for _, descriptor := range [][]byte{
		{0, 0, 0, 0, 3, 0x80, 0x80, 0x80, 0x80},
		{0, 0, 0, 0, 3, 10, 0},
		fixtureAudioConfiguration([]byte{0x12}, 0, nil),
		fixtureAudioConfiguration([]byte{0, 0}, 0, nil),
		fixtureAudioConfiguration([]byte{0x12, 0x10}, 0x40, []byte{255}),
	} {
		if _, err := mp4SampleCodec("mp4a", fixtureBox("esds", descriptor)); err == nil {
			t.Fatal("invalid audio configuration accepted")
		}
	}
}

func TestMP4AudioDescriptorPaddedLengths(t *testing.T) {
	// Real encoders commonly retain four-byte descriptor sizes with leading
	// continuation groups. They must produce the same codec as minimal lengths.
	decoder := make([]byte, 13)
	decoder[0] = 0x40
	decoder[1] = 5<<2 | 1
	decoder = append(decoder, []byte{5, 0x80, 0x80, 0x80, 2, 0x12, 0x10}...)
	elementaryStream := append([]byte{0, 1, 0, 4, 0x80, 0x80, 0x80, byte(len(decoder))}, decoder...)
	descriptor := append([]byte{0, 0, 0, 0, 3, 0x80, 0x80, 0x80, byte(len(elementaryStream))}, elementaryStream...)
	codec, err := mp4AudioCodec(descriptor)
	if err != nil || codec != "mp4a.40.2" {
		t.Fatalf("padded descriptor codec = %q, error = %v", codec, err)
	}
}
