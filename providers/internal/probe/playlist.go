package probe

import (
	"errors"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"net/url"
	"strconv"
	"strings"
)

// MediaSample identifies one segment and its optional byte range for bounded probing.
type MediaSample struct {
	URL         string
	RangeStart  int64
	RangeLength int64
}

// Playlist holds metadata, sampling coordinates, and DRM indicators parsed from an HLS manifest.
type Playlist struct {
	Samples               []MediaSample
	Metadata              model.Metadata
	SampleURL             string
	RangeStart            int64
	RangeLength           int64
	Encrypted             bool
	UnsupportedProtection bool
}

// attributes parses comma-separated key=value or key="value" attributes from an HLS tag line.
func attributes(attrStr string) (map[string]string, error) {
	result := make(map[string]string)
	isQuoted := false
	startIndex := 0
	var rawFields []string

	for i, ch := range attrStr {
		if ch == '"' {
			isQuoted = !isQuoted
		}
		if ch == ',' && !isQuoted {
			rawFields = append(rawFields, attrStr[startIndex:i])
			startIndex = i + 1
		}
	}

	if isQuoted {
		return nil, errors.New("unclosed HLS attribute quote")
	}
	rawFields = append(rawFields, attrStr[startIndex:])

	for _, field := range rawFields {
		parts := strings.SplitN(strings.TrimSpace(field), "=", 2)
		if len(parts) != 2 || parts[0] == "" {
			return nil, errors.New("invalid HLS attribute key-value pair")
		}

		key := parts[0]
		if _, exists := result[key]; exists {
			return nil, errors.New("duplicate HLS attribute key")
		}

		result[key] = strings.Trim(parts[1], `"`)
	}

	return result, nil
}

// resolveURI converts an HLS relative URI into an absolute validated URL.
func resolveURI(baseURL *url.URL, rawURI string) (string, error) {
	resolvedURL, err := baseURL.Parse(rawURI)
	if err != nil || network.ValidateURL(resolvedURL) != nil {
		return "", errors.New("invalid HLS resource URL")
	}
	return resolvedURL.String(), nil
}

// ParsePlaylist parses master and media playlists, capturing renditions, tracks, and segment coordinates.
func ParsePlaylist(body []byte, baseURL *url.URL) (Playlist, error) {
	playlist := Playlist{
		Metadata: model.Metadata{
			TracksKnown: true,
		},
	}

	manifestText := strings.TrimSpace(string(body))
	if !strings.HasPrefix(manifestText, "#EXTM3U\n") && !strings.HasPrefix(manifestText, "#EXTM3U\r\n") {
		return playlist, errors.New("invalid HLS header: missing #EXTM3U")
	}

	var pendingVariantAttrs map[string]string
	var pendingByteRange string
	segmentReady := false

	lines := strings.Split(manifestText, "\n")
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if len(line) > 16384 {
			return playlist, errors.New("HLS line length exceeds limit")
		}

		// Variant stream definition
		if rest, ok := strings.CutPrefix(line, "#EXT-X-STREAM-INF:"); ok {
			if pendingVariantAttrs != nil {
				return playlist, errors.New("missing variant URI following #EXT-X-STREAM-INF")
			}
			attrs, err := attributes(rest)
			if err != nil {
				return playlist, err
			}
			pendingVariantAttrs = attrs
			continue
		}

		// Audio/subtitle track definition
		if rest, ok := strings.CutPrefix(line, "#EXT-X-MEDIA:"); ok {
			attrs, err := attributes(rest)
			if err != nil {
				return playlist, err
			}

			trackType := attrs["TYPE"]
			if trackType != "AUDIO" && trackType != "SUBTITLES" {
				continue
			}

			track := model.Track{
				Kind:     strings.ToLower(trackType),
				Language: attrs["LANGUAGE"],
				Name:     attrs["NAME"],
				Group:    attrs["GROUP-ID"],
				Default:  attrs["DEFAULT"] == "YES",
			}

			if rawURI := attrs["URI"]; rawURI != "" {
				resolved, err := resolveURI(baseURL, rawURI)
				if err != nil {
					return playlist, err
				}
				track.URL = resolved
			}

			playlist.Metadata.Tracks = append(playlist.Metadata.Tracks, track)
			continue
		}

		// DRM / encryption tags
		if strings.HasPrefix(line, "#EXT-X-KEY:") || strings.HasPrefix(line, "#EXT-X-SESSION-KEY:") {
			tagParts := strings.SplitN(line, ":", 2)
			attrs, err := attributes(tagParts[1])
			if err != nil {
				return playlist, err
			}

			if method := attrs["METHOD"]; method != "NONE" {
				playlist.Encrypted = true
				if method != "AES-128" || (attrs["KEYFORMAT"] != "" && attrs["KEYFORMAT"] != "identity") {
					playlist.UnsupportedProtection = true
				}
			}
			continue
		}

		// Byte-range marker for following segment
		if rest, ok := strings.CutPrefix(line, "#EXT-X-BYTERANGE:"); ok {
			pendingByteRange = rest
			continue
		}

		// Segment marker
		if strings.HasPrefix(line, "#EXTINF:") {
			segmentReady = true
			continue
		}

		// URI line (either variant playlist URL or media segment URL)
		if line != "" && !strings.HasPrefix(line, "#") {
			resolvedURI, err := resolveURI(baseURL, line)
			if err != nil {
				return playlist, err
			}

			if pendingVariantAttrs != nil {
				variant := model.Variant{
					URL:           resolvedURI,
					AudioGroup:    pendingVariantAttrs["AUDIO"],
					SubtitleGroup: pendingVariantAttrs["SUBTITLES"],
				}

				bitrateStr := pendingVariantAttrs["AVERAGE-BANDWIDTH"]
				if bitrateStr == "" {
					bitrateStr = pendingVariantAttrs["BANDWIDTH"]
				}
				if bitrateStr != "" {
					parsedBitrate, err := strconv.ParseInt(bitrateStr, 10, 64)
					if err != nil || parsedBitrate <= 0 {
						return playlist, errors.New("invalid HLS bitrate")
					}
					variant.Bitrate = &parsedBitrate
				}

				if resStr := pendingVariantAttrs["RESOLUTION"]; resStr != "" {
					dims := strings.Split(resStr, "x")
					if len(dims) != 2 {
						return playlist, errors.New("invalid HLS resolution")
					}
					width, err := strconv.Atoi(dims[0])
					if err != nil || width <= 0 {
						return playlist, errors.New("invalid HLS width")
					}
					height, err := strconv.Atoi(dims[1])
					if err != nil || height <= 0 {
						return playlist, errors.New("invalid HLS height")
					}
					variant.Width = &width
					variant.Height = &height
				}

				if pendingVariantAttrs["CODECS"] != "" {
					variant.Codecs = strings.Split(pendingVariantAttrs["CODECS"], ",")
				}

				playlist.Metadata.Variants = append(playlist.Metadata.Variants, variant)
				pendingVariantAttrs = nil
			} else if segmentReady {
				sample := MediaSample{
					URL: resolvedURI,
				}

				if pendingByteRange != "" {
					rangeParts := strings.Split(pendingByteRange, "@")
					if len(rangeParts) > 2 {
						return playlist, errors.New("invalid byte range specification")
					}

					rangeLength, err := strconv.ParseInt(rangeParts[0], 10, 64)
					if err != nil || rangeLength <= 0 {
						return playlist, errors.New("invalid byte range length")
					}

					var rangeOffset int64
					if len(rangeParts) == 2 {
						rangeOffset, err = strconv.ParseInt(rangeParts[1], 10, 64)
						if err != nil || rangeOffset < 0 {
							return playlist, errors.New("invalid byte range offset")
						}
					} else {
						// Implicit offset follows contiguous previous sample
						if len(playlist.Samples) == 0 {
							return playlist, errors.New("initial byte range requires explicit offset (@)")
						}
						previousSample := playlist.Samples[len(playlist.Samples)-1]
						if previousSample.URL != resolvedURI || previousSample.RangeLength == 0 {
							return playlist, errors.New("implicit byte range requires preceding range on same resource")
						}
						rangeOffset = previousSample.RangeStart + previousSample.RangeLength
					}

					if rangeOffset+rangeLength < rangeOffset {
						return playlist, errors.New("byte range overflow")
					}

					sample.RangeStart = rangeOffset
					sample.RangeLength = rangeLength
				}

				if len(playlist.Samples) == 0 {
					playlist.SampleURL = sample.URL
					playlist.RangeStart = sample.RangeStart
					playlist.RangeLength = sample.RangeLength
				}

				playlist.Samples = append(playlist.Samples, sample)
				segmentReady = false
				pendingByteRange = ""
			}
		}
	}

	if len(playlist.Metadata.Variants) > 32 || len(playlist.Metadata.Tracks) > 64 {
		return playlist, errors.New("HLS metadata variant/track count limit exceeded")
	}

	return playlist, nil
}
