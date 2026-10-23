package vidcore

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCapturedPlayerTranslation(t *testing.T) {
	dir := os.Getenv("VIDCORE_CAPTURE_DIR")
	if dir == "" {
		t.Skip("private captured player is an opt-in research input")
	}
	source, e := os.ReadFile(filepath.Join(dir, "chunk-07.js"))
	if e != nil {
		t.Fatal(e)
	}
	p, e := compileProtocol(string(source))
	if e != nil {
		t.Fatal(e)
	}
	if p.ChallengeAction == "" || p.ListAction == "" || p.UnlockAction == "" {
		t.Fatal("request action missing")
	}
	// Never log decoded keys, session material, source URLs or captured response text.
	changed := strings.Replace(string(source), "Math.imul(s^s>>>16,0x85ebca6b)", "Math.imul(s^s>>>16,0x85ebca6a)", 1)
	if _, e := compileProtocol(changed); e == nil {
		t.Fatal("changed player material accepted")
	}
	t.Log("recognized both recovered instruction versions and extracted native protocol parameters")
}
