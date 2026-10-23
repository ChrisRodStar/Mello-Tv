package main

import "testing"

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
