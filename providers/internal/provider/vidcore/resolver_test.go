package vidcore

import (
	"family-stream-providers/internal/model"
	"os"
	"testing"
)

func TestEmbedFixtureAndMalformed(t *testing.T) {
	body, e := os.ReadFile("../../../testdata/providers/vidcore/embed.html")
	if e != nil {
		t.Fatal(e)
	}
	embed, e := ParseEmbed(string(body))
	if e != nil || embed.Token != "synthetic-session" || len(embed.Chunks) != 1 {
		t.Fatalf("unexpected embed parsing: %v", e)
	}
	if _, e := ParseEmbed("<html>no session</html>"); model.Category(e) != model.ProtocolChange {
		t.Fatal("missing token must be protocol change")
	}
}
func TestMaterialFixtureAndMalformed(t *testing.T) {
	body, e := os.ReadFile("../../../testdata/providers/vidcore/structure.txt")
	if e != nil {
		t.Fatal(e)
	}
	m, e := ParseMaterial(string(body))
	if e != nil || m.Base != "/fixture" || m.Action != "unlock" || m.RequireName != "req" {
		t.Fatalf("unexpected structural parse: %v", e)
	}
	if _, e := ParseMaterial("var changed=1;"); model.Category(e) != model.ProtocolChange {
		t.Fatal("changed structure must fail")
	}
}
func TestBindingXOR(t *testing.T) {
	b, e := ParseBindings(`v[[100,111].map(function(n){return String.fromCharCode(1^n)}).join("")]=session;`)
	if e != nil || len(b) != 1 || b[0].Name != "en" || b[0].Identifier != "session" {
		t.Fatal("native binding transform failed")
	}
}
func TestIdentifierMapping(t *testing.T) {
	season, episode := 0, 1
	cases := []struct {
		title model.Title
		path  string
	}{{model.Title{Type: model.Movie, TMDBID: 1198994}, "/movie/1198994"}, {model.Title{Type: model.TV, TMDBID: 44217, Season: &season, Episode: &episode}, "/tv/44217/0/1"}}
	for _, tc := range cases {
		path, e := embedPath(tc.title)
		if e != nil || path != tc.path {
			t.Fatal("incorrect ID mapping")
		}
	}
	if _, e := embedPath(model.Title{Type: model.TV, TMDBID: 44217}); model.Category(e) != model.Unsupported {
		t.Fatal("missing episode must fail")
	}
}
