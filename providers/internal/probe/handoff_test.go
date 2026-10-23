package probe

import (
	"net/url"
	"strings"
	"testing"
)

func TestRestrictedMasterPreservesAssociatedRenditionAttributes(t *testing.T) {
	master := `#EXTM3U
#EXT-X-VERSION:6
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",LANGUAGE="en",DEFAULT=YES,AUTOSELECT=YES,URI="audio/en.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",LANGUAGE="en",DEFAULT=NO,AUTOSELECT=YES,FORCED=YES,URI="subs/en.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="other",NAME="French",LANGUAGE="fr",URI="audio/fr.m3u8"
#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID="cc",NAME="CC",INSTREAM-ID="CC1"
#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1280x720,AUDIO="other"
720.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080,AUDIO="audio",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
1080.m3u8
`
	baseURL, _ := url.Parse("https://example.com/path/master.m3u8")
	filtered, err := SelectMasterVariant([]byte(master), baseURL, "https://example.com/path/1080.m3u8")
	if err != nil {
		t.Fatal(err)
	}
	text := string(filtered)
	for _, required := range []string{"1080.m3u8", "audio/en.m3u8", "subs/en.m3u8", "DEFAULT=NO,AUTOSELECT=YES,FORCED=YES", "INSTREAM-ID=\"CC1\"", "#EXT-X-INDEPENDENT-SEGMENTS"} {
		if !strings.Contains(text, required) {
			t.Fatalf("playable master missing %q", required)
		}
	}
	if strings.Count(text, "#EXT-X-STREAM-INF:") != 1 || strings.Contains(text, "720.m3u8") || strings.Contains(text, "audio/fr.m3u8") {
		t.Fatal("unselected rendition/group remained in playable master")
	}
}

func TestRestrictedMasterFailsIfSelectionChangedOrIsAmbiguous(t *testing.T) {
	baseURL, _ := url.Parse("https://example.com/master.m3u8")
	for _, manifest := range []string{
		"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\nother.m3u8\n",
		"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\nvideo.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000\nvideo.m3u8\n",
		"#EXTM3U\n#EXTINF:6,\nsegment.ts\n",
		"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,AUDIO=\"missing\"\nvideo.m3u8\n",
	} {
		if _, err := SelectMasterVariant([]byte(manifest), baseURL, "https://example.com/video.m3u8"); err == nil {
			t.Fatal("changed or ambiguous master accepted")
		}
	}
}
