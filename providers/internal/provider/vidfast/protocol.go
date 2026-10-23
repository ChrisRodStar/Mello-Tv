package vidfast

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"regexp"
	"strings"
	"time"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

const playerSHA256 = "97f52b92a9f7a1c6066f8b6ff3e2ad79f283f2e44f603de379912963f10809d0"

var (
	rootPattern = regexp.MustCompile(`"(/zirlaku/u/[A-Za-z0-9_-]{32,512})"`)
	csrfPattern = regexp.MustCompile(`"X-Csrf-Token":"([A-Za-z0-9_-]{16,128})"`)
)

type protocolMaterial struct {
	root string
	csrf string
}

func recognizePlayer(body []byte) (protocolMaterial, error) {
	return recognizePlayerVersion(body, playerSHA256)
}

// recognizePlayerVersion keeps fingerprint verification and extraction together.
// The production entry point pins playerSHA256; offline fixtures supply their
// own known digest through the package-private adapter test seam.
func recognizePlayerVersion(body []byte, expectedSHA256 string) (protocolMaterial, error) {
	digest := sha256.Sum256(body)
	if hex.EncodeToString(digest[:]) != expectedSHA256 {
		return protocolMaterial{}, protocolError("bundle", "unrecognized static player version")
	}
	roots := rootPattern.FindAllSubmatch(body, -1)
	headers := csrfPattern.FindAllSubmatch(body, -1)
	if len(roots) == 0 || len(headers) == 0 {
		return protocolMaterial{}, protocolError("bundle", "recognized protocol material missing")
	}
	material := protocolMaterial{
		root: string(roots[0][1]),
		csrf: string(headers[0][1]),
	}
	for _, match := range roots {
		if string(match[1]) != material.root {
			return protocolMaterial{}, protocolError("bundle", "ambiguous protocol origin path")
		}
	}
	for _, match := range headers {
		if string(match[1]) != material.csrf {
			return protocolMaterial{}, protocolError("bundle", "ambiguous protocol header")
		}
	}
	return material, nil
}

func (a *Adapter) protocolFetch(ctx context.Context, targetURL, method string, material protocolMaterial, limit int64) (network.Response, error) {
	response, err := a.client.Fetch(ctx, network.Request{
		URL:    targetURL,
		Method: method,
		Limit:  limit,
		Context: model.FetchContext{
			Scopes: []model.FetchScope{
				{
					Origin: origin,
					Headers: map[string]string{
						"Referer": origin + "/",
					},
				},
				{
					Origin:     origin,
					PathPrefix: material.root,
					Headers:    protocolHeaders(material),
				},
			},
		},
	})
	if err != nil {
		return network.Response{}, &requestError{cause: err}
	}
	if response.URL == nil || response.URL.String() != targetURL {
		return network.Response{}, protocolError("protocol", "upstream endpoint changed")
	}
	return response, nil
}

func protocolHeaders(material protocolMaterial) map[string]string {
	if material.csrf == "" {
		return nil
	}
	return map[string]string{"X-Csrf-Token": material.csrf}
}

type server struct {
	Name string `json:"name"`
	Data string `json:"data"`
}

func (a *Adapter) probeServers(ctx context.Context, bootstrap bootstrap, material protocolMaterial) ([]server, error) {
	salt := make([]byte, 16)
	if _, err := rand.Read(salt); err != nil {
		return nil, model.Fail(model.Unavailable, "request_codec", "secure random source unavailable")
	}
	packed, err := packRequest(bootstrap.Token, salt, time.Now().UnixMilli())
	if err != nil {
		return nil, protocolError("request_codec", "request encoding rejected")
	}
	challenge, err := a.protocolFetch(ctx, origin+material.root+"/JgQXcWPNH7s/"+packed, http.MethodPost, material, 4096)
	if err != nil {
		return nil, err
	}
	if challenge.Status != http.StatusOK {
		// The player has a fallback for non-200 responses. Its semantics have
		// not been verified; do not invent a fallback token or hide rejection.
		return nil, model.Fail(model.UpstreamFailure, "challenge", "upstream challenge response rejected")
	}
	token, err := challengeToken(challenge.Body)
	if err != nil {
		return nil, protocolError("challenge", "challenge transformation rejected")
	}
	response, err := a.protocolFetch(ctx, origin+material.root+"/dBWKuO8aW6U/"+token, http.MethodPost, material, 256*1024)
	if err != nil {
		return nil, err
	}
	if response.Status != http.StatusOK {
		return nil, model.Fail(model.UpstreamFailure, "servers", "upstream server list rejected")
	}
	plaintext, err := decryptEnvelope(string(response.Body), responseKey())
	if err != nil {
		return nil, protocolError("servers", "server list authentication or framing rejected")
	}
	var servers []server
	if json.Unmarshal(plaintext, &servers) != nil || len(servers) > 16 {
		return nil, protocolError("servers", "server list malformed or oversized")
	}
	for _, server := range servers {
		if server.Name == "" || len(server.Name) > 128 || strings.ContainsAny(server.Name, "\r\n\x00") || !opaqueToken(server.Data, 8192) {
			return nil, protocolError("servers", "server identity or unlock token malformed")
		}
	}
	return servers, nil
}

func opaqueToken(value string, limit int) bool {
	if len(value) == 0 || len(value) > limit {
		return false
	}
	for _, character := range value {
		if !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '-' || character == '_') {
			return false
		}
	}
	return true
}

func (a *Adapter) unlock(ctx context.Context, material protocolMaterial, server server) ([]byte, error) {
	response, err := a.protocolFetch(ctx, origin+material.root+"/M6SyMXJBpeg/"+server.Data, http.MethodPost, material, 256*1024)
	if err != nil {
		return nil, err
	}
	if response.Status == http.StatusNotFound || response.Status == http.StatusGone {
		return nil, model.Fail(model.Unavailable, "unlock", "server source unavailable")
	}
	if response.Status != http.StatusOK {
		return nil, model.Fail(model.UpstreamFailure, "unlock", "upstream server unlock rejected")
	}
	plaintext, err := decryptEnvelope(string(response.Body), responseKey())
	if err != nil {
		return nil, protocolError("unlock", "source authentication or framing rejected")
	}
	return plaintext, nil
}
