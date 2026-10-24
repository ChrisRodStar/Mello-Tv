package vidcore

import (
	"context"
	"errors"
	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"net/http"
	"net/http/cookiejar"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf16"
)

const (
	origin    = "https://vidcore.io"
	userAgent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36"
)

var (
	tokenPattern     = regexp.MustCompile(`\\\"en\\\":\\\"([^\\\"]+)\\\"`)
	chunkPattern     = regexp.MustCompile(`/_next/static/chunks/[\w./%-]+\.js`)
	modulePattern    = regexp.MustCompile(`var ([\w$]+)=([\w$]+)\((\d+)\),[\w$]+=([\w$]+)\((\d+)\)\.Buffer;`)
	boundaryPattern  = regexp.MustCompile(`let [\w$]+=[\w$]+=>\{let [\w$]+=Math\.floor\(`)
	listPattern      = regexp.MustCompile(`var [\w$]+=0[;,](?:[\w$]+=function\(\)\{for\(var e=Array\(6\)|[\w$]+\[\[[\d,]+\]\.map\(function\([\w$]+\))`)
	routePattern     = regexp.MustCompile(`fetch\(""\.concat\("([^"]+)","/"\)\.concat\("([^"]+)","/"\)`)
	unlockPattern    = regexp.MustCompile(`let ([\w$]+)=\[\];var [\w$,]+=0[;,]`)
	unlockEndPattern = regexp.MustCompile(`;await ([\w$]+),[\w$]+\.current=`)
	bindingPattern   = regexp.MustCompile(`[\w$]+\[\[([\d,]+)\]\.map\(function\([\w$]+\)\{return String\.fromCharCode\((\d+)\^[\w$]+\)\}\)\.join\(""\)\]=([^,;]+)`)
)

// Embed contains the session token and referenced Next.js chunk paths parsed from embed HTML.
type Embed struct {
	Token  string
	Chunks []string
}

// Material captures top-level structural signatures extracted from client JavaScript.
type Material struct {
	Base         string
	Action       string
	RequireName  string
	CryptoModule string
	BufferModule string
}

// ParseEmbed extracts the escaped session token and static script chunk URLs from embed HTML.
func ParseEmbed(htmlContent string) (Embed, error) {
	tokenMatches := tokenPattern.FindStringSubmatch(htmlContent)
	if len(tokenMatches) != 2 {
		return Embed{}, model.Fail(model.ProtocolChange, "embed", "escaped session token missing")
	}

	seenChunks := make(map[string]bool)
	var chunks []string
	for _, chunkPath := range chunkPattern.FindAllString(htmlContent, -1) {
		if !seenChunks[chunkPath] {
			seenChunks[chunkPath] = true
			chunks = append(chunks, chunkPath)
		}
	}

	if len(chunks) == 0 || len(chunks) > 32 {
		return Embed{}, model.Fail(model.ProtocolChange, "embed", "player chunk count outside supported bounds")
	}

	return Embed{
		Token:  tokenMatches[1],
		Chunks: chunks,
	}, nil
}

// ParseMaterial statically inspects a JavaScript chunk for expected protocol definitions and route constants.
func ParseMaterial(source string) (Material, error) {
	moduleIndices := modulePattern.FindStringSubmatchIndex(source)
	boundaryIndices := boundaryPattern.FindStringIndex(source)

	unsupported := func() (Material, error) {
		return Material{}, model.Fail(model.ProtocolChange, "player", "player structure unsupported")
	}

	if moduleIndices == nil || boundaryIndices == nil || boundaryIndices[0] < moduleIndices[0] {
		return unsupported()
	}

	capture := func(idx int) string {
		return source[moduleIndices[2*idx]:moduleIndices[2*idx+1]]
	}

	if capture(2) != capture(4) || !strings.Contains(source[moduleIndices[0]:boundaryIndices[0]], "JSON.parse('[") {
		return unsupported()
	}

	startIndices := listPattern.FindStringIndex(source[boundaryIndices[0]:])
	if startIndices == nil {
		return unsupported()
	}

	listStart := boundaryIndices[0] + startIndices[0]
	delimiterIndex := strings.Index(source[listStart:], ",()=>{clearInterval(")
	if delimiterIndex < 0 {
		return unsupported()
	}

	listEnd := listStart + delimiterIndex
	tail := source[listEnd:]

	routeMatches := routePattern.FindStringSubmatch(tail)
	unlockIndices := unlockPattern.FindStringIndex(tail)
	if routeMatches == nil || unlockIndices == nil || unlockEndPattern.FindStringIndex(tail[unlockIndices[0]:]) == nil {
		return unsupported()
	}

	if !strings.HasPrefix(routeMatches[1], "/") || strings.HasPrefix(routeMatches[1], "//") {
		return unsupported()
	}

	return Material{
		Base:         routeMatches[1],
		Action:       routeMatches[2],
		RequireName:  capture(2),
		CryptoModule: capture(3),
		BufferModule: capture(5),
	}, nil
}

// upstreamError converts network and context errors into categorized pipeline failures.
func upstreamError(ctx context.Context, stage string, cause error) error {
	if errors.Is(cause, context.DeadlineExceeded) || errors.Is(cause, context.Canceled) {
		return cause
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	return model.Fail(model.UpstreamFailure, stage, "upstream request or bounded read failed")
}

// resolve executes the full embed retrieval, chunk analysis, and protocol resolution flow.
func (a *Adapter) resolve(ctx context.Context, embedPath string, title model.Title) (model.AdapterResult, error) {
	jar, _ := cookiejar.New(nil)
	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin: origin,
				Headers: map[string]string{
					"User-Agent":      userAgent,
					"Accept":          "*/*",
					"Accept-Language": "en-US,en;q=0.9",
				},
			},
		},
	}

	response, err := a.client.Fetch(ctx, network.Request{
		URL:     origin + embedPath,
		Context: fetchContext,
		Jar:     jar,
		Limit:   5_000_000,
	})
	if err != nil {
		return model.AdapterResult{}, upstreamError(ctx, "embed", err)
	}

	if response.Status == http.StatusNotFound || response.Status == http.StatusGone {
		return model.AdapterResult{}, model.Fail(model.Unavailable, "embed", "title embed unavailable")
	}

	if response.Status < 200 || response.Status >= 300 || network.Origin(response.URL) != network.Origin(mustOrigin()) {
		return model.AdapterResult{}, model.Fail(model.UpstreamFailure, "embed", "embed response rejected")
	}

	embed, err := ParseEmbed(string(response.Body))
	if err != nil {
		return model.AdapterResult{}, err
	}

	// Consult all freshly declared chunk identities before downloading framework
	// chunks that precede a previously validated player in the embed.
	for _, chunkRelPath := range candidateChunks(embed.Chunks) {
		chunkURL := origin + chunkRelPath
		if cached, found := a.protocols.Get(chunkURL); found {
			return a.resolveNative(ctx, cached, embed, chunkURL, jar, title)
		}
	}
	// Cold discovery prioritizes feature/player chunks over framework runtimes.
	for _, chunkRelPath := range candidateChunks(embed.Chunks) {
		chunkURL := origin + chunkRelPath
		chunkResp, err := a.client.Fetch(ctx, network.Request{
			URL:     chunkURL,
			Context: fetchContext,
			Limit:   2_500_000,
		})
		if err != nil {
			return model.AdapterResult{}, upstreamError(ctx, "player", err)
		}

		if network.Origin(chunkResp.URL) != network.Origin(mustOrigin()) {
			return model.AdapterResult{}, model.Fail(model.UpstreamFailure, "player", "player origin changed")
		}

		if chunkResp.Status < 200 || chunkResp.Status >= 300 {
			continue
		}

		if _, err := ParseMaterial(string(chunkResp.Body)); err == nil {
			proto, err := compileProtocol(string(chunkResp.Body))
			if err != nil {
				return model.AdapterResult{}, err
			}
			a.protocols.Put(chunkURL, proto, cache.AssetLifetime(chunkResp.Header, 24*time.Hour), 8)
			return a.resolveNative(ctx, proto, embed, chunkURL, jar, title)
		}
	}

	return model.AdapterResult{}, model.Fail(model.ProtocolChange, "player", "no supported player structure found")
}

// candidateChunks orders chunks to prioritize application feature scripts over framework runtimes.
func candidateChunks(chunks []string) []string {
	var prioritized []string
	var framework []string

	for _, relPath := range chunks {
		lower := strings.ToLower(relPath)
		if strings.Contains(lower, "webpack") ||
			strings.Contains(lower, "framework") ||
			strings.Contains(lower, "main-app") ||
			strings.Contains(lower, "main-") ||
			strings.Contains(lower, "main.") ||
			strings.Contains(lower, "polyfills") ||
			strings.Contains(lower, "layout") {
			framework = append(framework, relPath)
		} else {
			prioritized = append(prioritized, relPath)
		}
	}

	for i, j := 0, len(prioritized)-1; i < j; i, j = i+1, j-1 {
		prioritized[i], prioritized[j] = prioritized[j], prioritized[i]
	}

	return append(prioritized, framework...)
}

func mustOrigin() *url.URL {
	parsed, _ := url.Parse(origin)
	return parsed
}

// Binding represents an obfuscated variable binding extracted without script execution.
type Binding struct {
	Name       string
	Identifier string
}

// ParseBindings decodes XOR-obfuscated object member assignments.
func ParseBindings(code string) ([]Binding, error) {
	var out []Binding

	for _, match := range bindingPattern.FindAllStringSubmatch(code, -1) {
		xorKey, err := strconv.ParseUint(match[2], 10, 32)
		if err != nil {
			return nil, model.Fail(model.ProtocolChange, "bindings", "invalid XOR key")
		}

		var utf16Units []uint16
		for _, rawCode := range strings.Split(match[1], ",") {
			charVal, err := strconv.ParseUint(rawCode, 10, 32)
			if err != nil {
				return nil, model.Fail(model.ProtocolChange, "bindings", "invalid character code")
			}
			utf16Units = append(utf16Units, uint16(charVal^xorKey))
		}

		out = append(out, Binding{
			Name:       string(utf16.Decode(utf16Units)),
			Identifier: match[3],
		})
	}

	return out, nil
}
