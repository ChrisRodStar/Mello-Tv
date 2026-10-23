package probe

import (
	"net/url"
	"testing"
)

func TestVariantsAndTracks(t *testing.T) {
	base, _ := url.Parse("https://example.com/master.m3u8")
	pl, e := ParsePlaylist([]byte("#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English, Stereo\",LANGUAGE=\"en\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720,CODECS=\"avc1.4d401f,mp4a.40.2\",AUDIO=\"a\"\n720.m3u8\n"), base)
	if e != nil || len(pl.Metadata.Variants) != 1 || len(pl.Metadata.Tracks) != 1 {
		t.Fatalf("playlist metadata: %v", e)
	}
	v := pl.Metadata.Variants[0]
	if *v.Height != 720 || len(v.Codecs) != 2 || v.URL != "https://example.com/720.m3u8" {
		t.Fatal("variant context lost")
	}
}
func TestByteRangeAndPrivateResources(t *testing.T) {
	base, _ := url.Parse("https://example.com/media.m3u8")
	pl, e := ParsePlaylist([]byte("#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nmedia.ts\n"), base)
	if e != nil || pl.RangeStart != 500 || pl.RangeLength != 1000 {
		t.Fatalf("range parsing: %v", e)
	}
	for _, text := range []string{"#EXTM3U\n#EXTINF:4,\nhttp://127.0.0.1/media\n", "#EXTM3U\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nmedia.ts\n", "not a playlist"} {
		if _, e := ParsePlaylist([]byte(text), base); e == nil {
			t.Fatal("malformed or unsafe playlist accepted")
		}
	}
}

func TestSubsequentImplicitByteRanges(t *testing.T) {
	baseURL, _ := url.Parse("https://example.com/media.m3u8")
	playlist, err := ParsePlaylist([]byte("#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:2000\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:3000\n#EXTINF:4,\nmedia.ts\n"), baseURL)
	if err != nil {
		t.Fatal(err)
	}
	if len(playlist.Samples) != 3 || playlist.Samples[1].RangeStart != 1500 || playlist.Samples[2].RangeStart != 3500 {
		t.Fatalf("consecutive range offsets incorrect: %+v", playlist.Samples)
	}
	for _, manifest := range []string{
		"#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nfirst.ts\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nother.ts\n",
		"#EXTM3U\n#EXT-X-BYTERANGE:1000@9223372036854775800\n#EXTINF:4,\nmedia.ts\n",
	} {
		if _, err := ParsePlaylist([]byte(manifest), baseURL); err == nil {
			t.Fatal("ambiguous or overflowing byte range accepted")
		}
	}
}
