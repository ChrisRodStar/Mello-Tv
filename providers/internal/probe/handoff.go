package probe

import (
	"errors"
	"net/url"
	"strings"
)

// SelectMasterVariant filters an HLS master playlist, keeping only the chosen video rendition
// and its associated audio, subtitle, and closed-caption rendition groups.
//
// Preserving original EXT-X-MEDIA attributes (such as DEFAULT, AUTOSELECT, and FORCED) ensures
// that player track selection behaviors remain intact without synthesizing incomplete tags.
func SelectMasterVariant(masterBody []byte, baseURL *url.URL, selectedVariantURL string) ([]byte, error) {
	playlist, err := ParsePlaylist(masterBody, baseURL)
	if err != nil {
		return nil, err
	}

	matchingVariants := 0
	for _, variant := range playlist.Metadata.Variants {
		if variant.URL == selectedVariantURL {
			matchingVariants++
		}
	}
	if matchingVariants != 1 {
		return nil, errors.New("selected HLS rendition absent or ambiguous in master playlist")
	}

	lines := strings.Split(strings.TrimSpace(string(masterBody)), "\n")
	selectedStreamInfTag := ""
	selectedStreamURI := ""
	renditionGroupIDs := make(map[string]string)
	pendingStreamInfTag := ""

	for _, rawLine := range lines {
		trimmedLine := strings.TrimSpace(rawLine)
		if strings.HasPrefix(trimmedLine, "#EXT-X-STREAM-INF:") {
			pendingStreamInfTag = trimmedLine
			continue
		}

		if trimmedLine == "" || strings.HasPrefix(trimmedLine, "#") || pendingStreamInfTag == "" {
			continue
		}

		resolvedURI, err := resolveURI(baseURL, trimmedLine)
		if err != nil {
			return nil, err
		}

		if resolvedURI == selectedVariantURL {
			selectedStreamInfTag = pendingStreamInfTag
			selectedStreamURI = trimmedLine

			variantAttrs, err := attributes(strings.TrimPrefix(pendingStreamInfTag, "#EXT-X-STREAM-INF:"))
			if err != nil {
				return nil, err
			}

			for _, trackKind := range []string{"AUDIO", "SUBTITLES", "VIDEO", "CLOSED-CAPTIONS"} {
				renditionGroupIDs[trackKind] = variantAttrs[trackKind]
			}
		}

		pendingStreamInfTag = ""
	}

	var filteredLines []string
	retainedGroupKinds := make(map[string]bool)
	isVariantLinePending := false

	for _, rawLine := range lines {
		trimmedLine := strings.TrimSpace(rawLine)
		if strings.HasPrefix(trimmedLine, "#EXT-X-STREAM-INF:") {
			isVariantLinePending = true
			continue
		}

		if trimmedLine != "" && !strings.HasPrefix(trimmedLine, "#") && isVariantLinePending {
			isVariantLinePending = false
			continue
		}

		if strings.HasPrefix(trimmedLine, "#EXT-X-I-FRAME-STREAM-INF:") {
			continue
		}

		if mediaAttrsText, found := strings.CutPrefix(trimmedLine, "#EXT-X-MEDIA:"); found {
			renditionAttrs, err := attributes(mediaAttrsText)
			if err != nil {
				return nil, err
			}

			trackType := renditionAttrs["TYPE"]
			requiredGroup := renditionGroupIDs[trackType]

			if requiredGroup == "" || requiredGroup == "NONE" || renditionAttrs["GROUP-ID"] != requiredGroup {
				continue
			}

			retainedGroupKinds[trackType] = true
		}

		filteredLines = append(filteredLines, trimmedLine)
	}

	for trackKind, requiredGroup := range renditionGroupIDs {
		if requiredGroup != "" && requiredGroup != "NONE" && !retainedGroupKinds[trackKind] {
			return nil, errors.New("selected HLS rendition group missing")
		}
	}

	filteredLines = append(filteredLines, selectedStreamInfTag, selectedStreamURI)

	return []byte(strings.Join(filteredLines, "\n") + "\n"), nil
}
