package main

import (
	"net/http"
	"testing"

	"stv-providers/internal/network"
)

func TestAttestationUsesExplicitFamilyEndpoints(t *testing.T) {
	for family, expected := range map[string]string{
		"ip4": "https://ipv4.am.i.mullvad.net/json",
		"ip6": "https://ipv6.am.i.mullvad.net/json",
	} {
		endpoint, err := attestationEndpoint(family)
		if err != nil || endpoint != expected {
			t.Fatal("attestation must use the endpoint for its forced address family")
		}
	}
	if _, err := attestationEndpoint("ip"); err == nil {
		t.Fatal("unspecified address family accepted")
	}
}

func TestAttestationRequiresSuccessfulPositiveExitAssertion(t *testing.T) {
	for _, response := range []network.Response{
		{Status: http.StatusForbidden, Body: []byte(`{"mullvad_exit_ip":true}`)},
		{Status: http.StatusOK, Body: []byte(`{"mullvad_exit_ip":false}`)},
		{Status: http.StatusOK, Body: []byte(`{}`)},
		{Status: http.StatusOK, Body: []byte(`invalid`)},
	} {
		if err := verifyAttestation(response); err == nil {
			t.Fatal("unattested response accepted")
		}
	}
	if err := verifyAttestation(network.Response{
		Status: http.StatusOK,
		Body:   []byte(`{"mullvad_exit_ip":true}`),
	}); err != nil {
		t.Fatal("attested response rejected")
	}
}
