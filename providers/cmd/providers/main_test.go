package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestListenerConfigurationAcceptsOnlyLiteralLoopbackWithoutResolution(t *testing.T) {
	for _, value := range []string{
		"localhost:8090",
		"source.example.com:8090",
		"192.168.1.1:8090",
		"0.0.0.0:8090",
		"[::]:8090",
		"127.0.0.1:0",
		"[::1%lo0]:8090",
	} {
		if _, err := listenAddress(value); err == nil {
			t.Fatal("nonliteral, nonloopback or invalid listener admitted")
		}
	}
	for _, value := range []string{"127.0.0.1:8090", "[::1]:8090"} {
		if _, err := listenAddress(value); err != nil {
			t.Fatal("literal loopback listener rejected")
		}
	}
}

func TestReadPrivateValidation(t *testing.T) {
	dir := t.TempDir()

	// Missing file
	if _, err := readPrivate(filepath.Join(dir, "missing.json"), 1024); err == nil || err.Error() != "private file unavailable" {
		t.Errorf("expected 'private file unavailable', got %v", err)
	}

	// Permissive mode 0644 (must fail)
	insecureFile := filepath.Join(dir, "insecure.json")
	if err := os.WriteFile(insecureFile, []byte("data"), 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := readPrivate(insecureFile, 1024); err == nil || err.Error() != "private file mode required" {
		t.Errorf("expected 'private file mode required', got %v", err)
	}

	// Empty file with 0600 (must fail: empty bound)
	emptyFile := filepath.Join(dir, "empty.json")
	if err := os.WriteFile(emptyFile, []byte{}, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := readPrivate(emptyFile, 1024); err == nil || err.Error() != "private file bound" {
		t.Errorf("expected 'private file bound' for empty file, got %v", err)
	}

	// Oversized file with 0600 (must fail: bound exceeded)
	oversizedFile := filepath.Join(dir, "oversized.json")
	if err := os.WriteFile(oversizedFile, []byte("1234567890"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := readPrivate(oversizedFile, 5); err == nil || err.Error() != "private file bound" {
		t.Errorf("expected 'private file bound' for oversized file, got %v", err)
	}

	// Valid file with 0600
	validFile := filepath.Join(dir, "valid.json")
	if err := os.WriteFile(validFile, []byte("{\"valid\":true}"), 0600); err != nil {
		t.Fatal(err)
	}
	data, err := readPrivate(validFile, 1024)
	if err != nil {
		t.Fatalf("unexpected error reading valid private file: %v", err)
	}
	if string(data) != "{\"valid\":true}" {
		t.Errorf("expected '{\"valid\":true}', got %s", string(data))
	}
}

func BenchmarkListenAddress(b *testing.B) {
	b.ReportAllocs()
	for b.Loop() {
		if _, err := listenAddress("127.0.0.1:8090"); err != nil {
			b.Fatal(err)
		}
	}
}

