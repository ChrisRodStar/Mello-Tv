package probe

import (
	"context"
	"encoding/json"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/selection"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"testing"
	"time"
)

// fixtureMediaClient returns synthetic media bytes without making network requests.
type fixtureMediaClient struct {
	requests  []network.Request
	failPath  string
	playlists map[string]string
}

func (client *fixtureMediaClient) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	client.requests = append(client.requests, request)
	parsedURL, err := url.Parse(request.URL)
	if err != nil {
		return network.Response{}, err
	}
	if manifest, found := client.playlists[parsedURL.Path]; found {
		return network.Response{
			URL:    parsedURL,
			Status: http.StatusOK,
			Header: http.Header{"Content-Type": []string{"application/vnd.apple.mpegurl"}},
			Body:   []byte(manifest),
		}, nil
	}
	status := http.StatusOK
	if parsedURL.Path == client.failPath {
		status = http.StatusServiceUnavailable
	}
	transfer := time.Duration(len(client.requests)) * 10 * time.Millisecond
	return network.Response{
		URL:       parsedURL,
		Status:    status,
		Header:    http.Header{"Content-Type": []string{"video/mp2t"}},
		Body:      make([]byte, min(request.Limit, 32*1024)),
		FirstByte: transfer,
		Transfer:  transfer,
	}, nil
}

func fixturePlaylist(t *testing.T) Playlist {
	t.Helper()
	baseURL, err := url.Parse("https://example.com/media.m3u8")
	if err != nil {
		t.Fatal(err)
	}
	playlist, err := ParsePlaylist([]byte("#EXTM3U\n#EXTINF:4,\nfirst.ts\n#EXTINF:4,\nsecond.ts\n#EXTINF:4,\nmiddle.ts\n#EXTINF:4,\nfourth.ts\n#EXTINF:4,\nlast.ts\n"), baseURL)
	if err != nil {
		t.Fatal(err)
	}
	return playlist
}

func TestProbeSamplesDistributedPositionsConservatively(t *testing.T) {
	client := &fixtureMediaClient{}
	pool := &Pool{client: client, limits: DefaultLimits()}
	candidate := model.Candidate{ID: "fixture", Format: model.HLS}
	budget := &byteBudget{remaining: 3 * pool.limits.SampleBytes}
	result := pool.sample(context.Background(), candidate, fixturePlaylist(t), nil, budget)

	var paths []string
	for _, request := range client.requests {
		parsedURL, _ := url.Parse(request.URL)
		paths = append(paths, parsedURL.Path)
		if !request.Sample || request.Limit != pool.limits.SampleBytes {
			t.Fatal("sample byte bounds were lost")
		}
	}
	if want := []string{"/first.ts", "/middle.ts", "/last.ts"}; !reflect.DeepEqual(paths, want) {
		t.Fatalf("sample paths = %v, want %v", paths, want)
	}
	observation := result.Observation
	if !observation.Accessible || observation.SamplesCompleted != 3 || observation.SampleBytes != 3*32*1024 {
		t.Fatalf("incomplete aggregate observation: %+v", observation)
	}
	wantSpeed := float64(32*1024-1) * 8 / (30 * time.Millisecond).Seconds()
	if observation.DeliveryBPS == nil || *observation.DeliveryBPS != wantSpeed {
		t.Fatal("aggregate did not retain slowest segment delivery")
	}
	if observation.FirstByte == nil || *observation.FirstByte != 30*time.Millisecond || budget.remaining != 0 {
		t.Fatal("response delay or aggregate byte budget was lost")
	}
}

func TestLaterSegmentFailureRejectsRendition(t *testing.T) {
	client := &fixtureMediaClient{failPath: "/middle.ts"}
	pool := &Pool{client: client, limits: DefaultLimits()}
	result := pool.sample(context.Background(), model.Candidate{}, fixturePlaylist(t), nil, &byteBudget{remaining: 1_000_000})
	if result.Observation.Accessible || result.Observation.Failure != model.UpstreamFailure || result.Observation.DeliveryBPS != nil {
		t.Fatal("later failed segment left rendition eligible")
	}
	if len(client.requests) != 2 || result.Observation.SamplesCompleted != 1 {
		t.Fatal("sampling continued after upstream rejection")
	}
}

func TestSampleBudgetExhaustionStopsFurtherTraffic(t *testing.T) {
	client := &fixtureMediaClient{}
	pool := &Pool{client: client, limits: DefaultLimits()}
	result := pool.sample(context.Background(), model.Candidate{}, fixturePlaylist(t), nil, &byteBudget{remaining: pool.limits.SampleBytes})
	if result.Observation.Failure != model.Unavailable || len(client.requests) != 1 {
		t.Fatal("sampling escaped aggregate byte budget")
	}
}

func TestVariantLimitProbesHighestResolutionFirst(t *testing.T) {
	client := &fixtureMediaClient{
		playlists: map[string]string{
			"/master.m3u8": "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=854x480\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080\nhigh.m3u8\n",
			"/high.m3u8":   "#EXTM3U\n#EXTINF:4,\nhigh.ts\n",
		},
	}
	limits := DefaultLimits()
	limits.MaxVariants = 1
	pool := &Pool{client: client, limits: limits}
	candidate := model.Candidate{URL: "https://example.com/master.m3u8", Format: model.HLS}
	results := pool.probe(context.Background(), candidate, &byteBudget{remaining: 1_000_000}, &byteBudget{remaining: 1_000_000})
	if len(results) != 1 || !results[0].Observation.Accessible || *results[0].Observation.Variant.Height != 1080 {
		t.Fatal("variant limit favored first manifest entry over highest resolution")
	}
}

func TestSelectedRenditionRetainsPlayableMasterAndRequiredTracks(t *testing.T) {
	master := "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI=\"subs.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\",AUDIO=\"audio\",SUBTITLES=\"subs\"\nvideo.m3u8\n"
	client := &fixtureMediaClient{playlists: map[string]string{
		"/master.m3u8": master,
		"/video.m3u8":  "#EXTM3U\n#EXTINF:6,\nvideo.ts\n",
	}}
	pool := &Pool{client: client, limits: DefaultLimits()}
	title := model.Title{Type: model.Movie, TMDBID: 1}
	candidate := model.Candidate{ID: "separate-tracks", Title: title, URL: "https://example.com/master.m3u8", Format: model.HLS}
	observations := pool.probe(context.Background(), candidate, &byteBudget{remaining: 1_000_000}, &byteBudget{remaining: 1_000_000})
	lookup := model.Lookup{
		Title:       title,
		Preferences: model.Preferences{RequireAudio: true, AudioLanguages: []string{"en"}, RequireSubtitles: true, SubtitleLanguages: []string{"en"}},
		Device:      model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1", "mp4a"}},
	}
	result := selection.Select(lookup, observations)
	if result.Selected == nil {
		t.Fatal("associated required tracks did not qualify")
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	var handoff model.Selection
	if err := json.Unmarshal(encoded, &handoff); err != nil {
		t.Fatal(err)
	}
	source := handoff.Selected.Candidate.Source
	if source.URL != candidate.URL || source.SelectedVariantURL != "https://example.com/video.m3u8" {
		t.Fatal("serialized playback handoff lost master/rendition association")
	}
	baseURL, _ := url.Parse(source.URL)
	filtered, err := SelectMasterVariant([]byte(master), baseURL, source.SelectedVariantURL)
	if err != nil {
		t.Fatal(err)
	}
	playable, err := ParsePlaylist(filtered, baseURL)
	if err != nil || len(playable.Metadata.Variants) != 1 || len(playable.Metadata.Tracks) != 2 {
		t.Fatal("playable selection lost separate required tracks")
	}
}

func TestProbeHeightCeilingAvoidsUnsupportedHighRenditionWork(t *testing.T) {
	client := &fixtureMediaClient{playlists: map[string]string{
		"/master.m3u8": "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=9000000,RESOLUTION=3840x2160,CODECS=\"avc1\"\n4k.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1920x1080,CODECS=\"avc1\"\nhd.m3u8\n",
		"/hd.m3u8":     "#EXTM3U\n#EXTINF:4,\none.ts\n#EXTINF:4,\ntwo.ts\n#EXTINF:4,\nthree.ts\n",
	}}
	pool := &Pool{client: client, limits: DefaultLimits()}
	candidate := model.Candidate{ID: "fixture", URL: "https://example.com/master.m3u8", Format: model.HLS}
	results := pool.probe(WithHeightLimit(context.Background(), 1080, 0), candidate, &byteBudget{remaining: 4 * 1024 * 1024}, &byteBudget{remaining: 4 * 1024 * 1024})
	if len(results) != 1 || results[0].Observation.Variant == nil || *results[0].Observation.Variant.Height != 1080 {
		t.Fatalf("unexpected ceiling result: %d", len(results))
	}
	for _, request := range client.requests {
		if strings.Contains(request.URL, "4k") {
			t.Fatal("spent probe budget above device ceiling")
		}
	}
}
