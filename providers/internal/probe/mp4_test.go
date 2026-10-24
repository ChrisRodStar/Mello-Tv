package probe

import (
	"context"
	"encoding/binary"
	"fmt"
	"net/http"
	"net/url"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

func fixtureBox(kind string, data []byte) []byte {
	box := make([]byte, 8+len(data))
	binary.BigEndian.PutUint32(box, uint32(len(box)))
	copy(box[4:], kind)
	copy(box[8:], data)
	return box
}

// The fixture uses actual ISO BMFF field positions; it is not playable media.
func fixtureMP4(codec string) []byte {
	entry := make([]byte, 78)
	binary.BigEndian.PutUint16(entry[24:], 1920)
	binary.BigEndian.PutUint16(entry[26:], 1080)
	entries := make([]byte, 8)
	binary.BigEndian.PutUint32(entries[4:], 1)
	entries = append(entries, fixtureBox(codec, entry)...)
	metadata := fixtureBox("stsd", entries)
	for _, kind := range []string{"stbl", "minf", "mdia", "trak", "moov"} {
		metadata = fixtureBox(kind, metadata)
	}
	file := append(fixtureBox("ftyp", []byte("isom\x00\x00\x00\x00isom")), metadata...)
	return append(file, fixtureBox("mdat", make([]byte, 512*1024))...)
}

type mp4FixtureClient struct {
	body         []byte
	starts       []int64
	ignoreRange  bool
	corruptRange bool
	failLater    bool
}

func (client *mp4FixtureClient) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	var start, end int64
	if _, err := fmt.Sscanf(request.Headers["Range"], "bytes=%d-%d", &start, &end); err != nil {
		return network.Response{}, err
	}
	client.starts = append(client.starts, start)
	end = min(end, int64(len(client.body))-1)
	parsedURL, err := url.Parse(request.URL)
	if err != nil {
		return network.Response{}, err
	}
	status := http.StatusPartialContent
	if client.ignoreRange {
		status = http.StatusOK
	}
	if client.failLater && start > 0 {
		status = http.StatusServiceUnavailable
	}
	rangeStart := start
	if client.corruptRange {
		rangeStart++
	}
	return network.Response{
		URL:    parsedURL,
		Status: status,
		Header: http.Header{
			"Content-Type":  []string{"video/mp4"},
			"Content-Range": []string{fmt.Sprintf("bytes %d-%d/%d", rangeStart, end, len(client.body))},
		},
		Body:      client.body[start : end+1],
		FirstByte: 10 * time.Millisecond,
		Transfer:  time.Duration(len(client.starts)) * 10 * time.Millisecond,
	}, nil
}

func TestMP4ProbesClearMetadataAndDistributedRanges(t *testing.T) {
	client := &mp4FixtureClient{body: fixtureMP4("avc1")}
	pool := &Pool{client: client, limits: DefaultLimits()}
	candidate := model.Candidate{
		URL:    "https://example.com/file.mp4",
		Format: model.MP4,
	}
	result := pool.probeMP4(context.Background(), candidate, &byteBudget{remaining: 1024 * 1024}, &byteBudget{remaining: 1024 * 1024})
	observation := result.Observation
	if !observation.Accessible || !observation.ProtectionKnown || observation.Format != model.MP4 || observation.SamplesCompleted != 3 {
		t.Fatalf("clear file did not pass: %+v", observation)
	}
	if observation.Variant == nil || *observation.Variant.Height != 1080 || *observation.Variant.Width != 1920 || observation.Variant.Codecs[0] != "avc1" {
		t.Fatal("MP4 track metadata was lost")
	}
	if len(client.starts) != 3 || client.starts[0] != 0 || client.starts[1] <= 0 || client.starts[2] <= client.starts[1] {
		t.Fatal("file was not sampled at distributed positions")
	}
	expectedSpeed := float64(pool.limits.SampleBytes-1) * 8 / (30 * time.Millisecond).Seconds()
	if observation.DeliveryBPS == nil || *observation.DeliveryBPS != expectedSpeed || observation.Metadata.TracksKnown {
		t.Fatal("conservative throughput or unknown languages changed")
	}
}

func TestMP4RejectsProtectedMalformedOrInaccessibleRanges(t *testing.T) {
	testCases := []struct {
		Name   string
		Client *mp4FixtureClient
	}{
		{Name: "protected", Client: &mp4FixtureClient{body: fixtureMP4("encv")}},
		{Name: "range-ignored", Client: &mp4FixtureClient{body: fixtureMP4("avc1"), ignoreRange: true}},
		{Name: "range-mismatch", Client: &mp4FixtureClient{body: fixtureMP4("avc1"), corruptRange: true}},
		{Name: "later-failure", Client: &mp4FixtureClient{body: fixtureMP4("avc1"), failLater: true}},
		{Name: "html", Client: &mp4FixtureClient{body: make([]byte, 512*1024)}},
	}
	for _, testCase := range testCases {
		t.Run(testCase.Name, func(t *testing.T) {
			pool := &Pool{client: testCase.Client, limits: DefaultLimits()}
			result := pool.probeMP4(context.Background(), model.Candidate{URL: "https://example.com/file.mp4", Format: model.MP4}, &byteBudget{remaining: 1024 * 1024}, &byteBudget{remaining: 1024 * 1024})
			if result.Observation.Accessible || result.Observation.Failure == "" || result.Observation.DeliveryBPS != nil {
				t.Fatal("unverified MP4 remained eligible")
			}
		})
	}
}

func TestMP4BudgetStopsLaterSamples(t *testing.T) {
	client := &mp4FixtureClient{body: fixtureMP4("avc1")}
	pool := &Pool{client: client, limits: DefaultLimits()}
	result := pool.probeMP4(context.Background(), model.Candidate{URL: "https://example.com/file.mp4"}, &byteBudget{remaining: pool.limits.SampleBytes}, &byteBudget{remaining: 1024 * 1024})
	if result.Observation.Accessible || result.Observation.Failure != model.Unavailable || len(client.starts) != 1 {
		t.Fatal("file probe escaped its sample budget")
	}
}

func TestMP4BoxBounds(t *testing.T) {
	for _, body := range [][]byte{{0}, {0, 0, 0, 1, 'm', 'o', 'o', 'v'}, {0, 0, 0, 7, 'f', 't', 'y', 'p'}, {0, 0, 1, 0, 'm', 'o', 'o', 'v'}} {
		if _, err := mp4Boxes(body); err == nil {
			t.Fatal("invalid MP4 box bounds accepted")
		}
	}
}

func TestMP4LargeSeekTablesDoNotConsumeMetadataBudget(t *testing.T) {
	entry := make([]byte, 78)
	binary.BigEndian.PutUint16(entry[24:], 1920)
	binary.BigEndian.PutUint16(entry[26:], 1080)
	descriptions := make([]byte, 8)
	binary.BigEndian.PutUint32(descriptions[4:], 1)
	descriptions = append(descriptions, fixtureBox("avc1", entry)...)
	videoTrack := fixtureBox("trak", fixtureBox("mdia", fixtureBox("minf", fixtureBox("stbl", fixtureBox("stsd", descriptions)))))
	// A large timing table is structurally skipped. A subsequent track forces a
	// remote metadata page read beyond the initial sample, without reading the table.
	timingTable := fixtureBox("free", make([]byte, 7*1024*1024))
	audioEntry := make([]byte, 28)
	audioDescriptions := make([]byte, 8)
	binary.BigEndian.PutUint32(audioDescriptions[4:], 1)
	audioDescriptions = append(audioDescriptions, fixtureBox("mp4a", audioEntry)...)
	audioTrack := fixtureBox("trak", fixtureBox("mdia", fixtureBox("minf", fixtureBox("stbl", fixtureBox("stsd", audioDescriptions)))))
	metadata := append(append(videoTrack, timingTable...), audioTrack...)
	file := append(fixtureBox("ftyp", []byte("isom\x00\x00\x00\x00isom")), fixtureBox("moov", metadata)...)
	file = append(file, fixtureBox("mdat", make([]byte, 512*1024))...)
	client := &mp4FixtureClient{body: file}
	pool := &Pool{client: client, limits: DefaultLimits()}
	metadataBudget := &byteBudget{remaining: 16 * 1024}
	result := pool.probeMP4(context.Background(), model.Candidate{URL: "https://example.com/file.mp4"}, &byteBudget{remaining: 1024 * 1024}, metadataBudget)
	if !result.Observation.Accessible || metadataBudget.remaining != 8192 || len(result.Observation.Variant.Codecs) != 2 {
		t.Fatalf("large tables were downloaded or track metadata lost: %+v", result.Observation)
	}
}
