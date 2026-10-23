package probe

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

type mp4Box struct {
	Kind string
	Data []byte
}

// mp4MetadataReader visits box headers and sample descriptions without reading
// large timing/seek tables. Cached 8 KiB pages and a fetch cap bound the work.
type mp4MetadataReader struct {
	pool      *Pool
	ctx       context.Context
	candidate model.Candidate
	budget    *byteBudget
	initial   []byte
	total     int64
	pages     map[int64][]byte
	fetches   int
	boxes     int
}

func (reader *mp4MetadataReader) read(start, length int64) ([]byte, error) {
	if start < 0 || length < 0 || start > reader.total || length > reader.total-start || length > 256*1024 {
		return nil, errors.New("MP4 metadata read outside bounds")
	}
	if start+length <= int64(len(reader.initial)) {
		return reader.initial[start : start+length], nil
	}
	result := make([]byte, 0, length)
	for int64(len(result)) < length {
		position := start + int64(len(result))
		pageStart := position / 8192 * 8192
		page, found := reader.pages[pageStart]
		if !found {
			if reader.fetches >= 32 {
				return nil, model.Fail(model.Unsupported, "mp4", "metadata fetch limit exceeded")
			}
			pageLength := min(int64(8192), reader.total-pageStart)
			if reader.budget.reserve(pageLength) != pageLength {
				return nil, model.Fail(model.Unavailable, "mp4", "metadata budget exhausted")
			}
			reader.fetches++
			response, _, err := reader.pool.mp4Range(reader.ctx, reader.candidate, pageStart, pageLength, reader.total)
			if err != nil {
				return nil, err
			}
			page = response.Body
			reader.pages[pageStart] = page
		}
		pageOffset := position - pageStart
		bytesToCopy := min(length-int64(len(result)), int64(len(page))-pageOffset)
		if bytesToCopy <= 0 {
			return nil, errors.New("truncated MP4 metadata page")
		}
		result = append(result, page[pageOffset:pageOffset+bytesToCopy]...)
	}
	return result, nil
}

func (reader *mp4MetadataReader) inspect(start, end int64, variant *model.Variant, depth int) error {
	if depth > 8 {
		return errors.New("MP4 metadata nesting exceeded")
	}
	for position := start; position < end; {
		reader.boxes++
		if reader.boxes > 4096 || end-position < 8 {
			return errors.New("MP4 metadata box bounds exceeded")
		}
		header, err := reader.read(position, 8)
		if err != nil {
			return err
		}
		size := uint64(binary.BigEndian.Uint32(header))
		headerSize := int64(8)
		if size == 1 {
			header, err = reader.read(position, 16)
			if err != nil {
				return err
			}
			size = binary.BigEndian.Uint64(header[8:])
			headerSize = 16
		}
		if size == 0 {
			size = uint64(end - position)
		}
		if size < uint64(headerSize) || size > uint64(end-position) {
			return errors.New("MP4 child box outside parent bounds")
		}
		boxEnd := position + int64(size)
		switch string(header[4:8]) {
		case "trak", "mdia", "minf", "stbl":
			if err := reader.inspect(position+headerSize, boxEnd, variant, depth+1); err != nil {
				return err
			}
		case "stsd":
			description, err := reader.read(position, int64(size))
			if err != nil {
				return err
			}
			if err := inspectMP4Metadata(description, variant, 0); err != nil {
				return err
			}
		case "pssh", "sinf", "senc", "saiz", "saio":
			return errors.New("protected MP4 unsupported")
		}
		position = boxEnd
	}
	return nil
}

// mp4Boxes reads complete ISO BMFF boxes only. Extended sizes and zero-sized
// terminal boxes are bounded by the already limited parent slice.
func mp4Boxes(body []byte) ([]mp4Box, error) {
	var boxes []mp4Box
	for len(body) > 0 {
		if len(body) < 8 || len(boxes) >= 4096 {
			return nil, errors.New("invalid MP4 box bounds")
		}
		size := uint64(binary.BigEndian.Uint32(body))
		headerSize := uint64(8)
		if size == 1 {
			if len(body) < 16 {
				return nil, errors.New("truncated extended MP4 box")
			}
			size = binary.BigEndian.Uint64(body[8:])
			headerSize = 16
		}
		if size == 0 {
			size = uint64(len(body))
		}
		if size < headerSize || size > uint64(len(body)) {
			return nil, errors.New("truncated MP4 box")
		}
		boxes = append(boxes, mp4Box{
			Kind: string(body[4:8]),
			Data: body[headerSize:size],
		})
		body = body[size:]
	}
	return boxes, nil
}

// inspectMP4Metadata accepts clear, recognized sample entries in a complete
// moov box. Unknown entries, encrypted tracks and unrecognized structural
// extensions fail closed rather than asserting protection or codec support.
func inspectMP4Metadata(body []byte, variant *model.Variant, depth int) error {
	if depth > 8 {
		return errors.New("MP4 nesting limit exceeded")
	}
	boxes, err := mp4Boxes(body)
	if err != nil {
		return err
	}
	for _, box := range boxes {
		switch box.Kind {
		case "trak", "mdia", "minf", "stbl":
			if err := inspectMP4Metadata(box.Data, variant, depth+1); err != nil {
				return err
			}
		case "pssh", "sinf", "senc", "saiz", "saio":
			return errors.New("protected MP4 unsupported")
		case "stsd":
			if len(box.Data) < 8 || binary.BigEndian.Uint32(box.Data) != 0 {
				return errors.New("unsupported MP4 sample description")
			}
			entries, err := mp4Boxes(box.Data[8:])
			if err != nil || len(entries) != int(binary.BigEndian.Uint32(box.Data[4:])) || len(entries) != 1 {
				return errors.New("ambiguous MP4 sample descriptions")
			}
			entry := entries[0]
			codec := entry.Kind
			switch entry.Kind {
			case "avc1", "avc3", "hvc1", "hev1":
				if len(entry.Data) < 78 || variant.Height != nil {
					return errors.New("invalid or multiple MP4 video tracks")
				}
				width := int(binary.BigEndian.Uint16(entry.Data[24:]))
				height := int(binary.BigEndian.Uint16(entry.Data[26:]))
				if width == 0 || height == 0 {
					return errors.New("invalid MP4 dimensions")
				}
				variant.Width = &width
				variant.Height = &height
				if err := inspectMP4Metadata(entry.Data[78:], variant, depth+1); err != nil {
					return err
				}
				codec, err = mp4SampleCodec(entry.Kind, entry.Data[78:])
			case "mp4a", "ac-3", "ec-3":
				if len(entry.Data) < 28 || binary.BigEndian.Uint16(entry.Data[8:]) != 0 {
					return errors.New("unsupported MP4 audio entry version")
				}
				if err := inspectMP4Metadata(entry.Data[28:], variant, depth+1); err != nil {
					return err
				}
				codec, err = mp4SampleCodec(entry.Kind, entry.Data[28:])
			case "text":
				// Observed QuickTime text/chapter descriptions do not establish
				// playable subtitles or an audio/video codec requirement.
				if len(entry.Data) < 8 {
					return errors.New("truncated QuickTime text description")
				}
				continue
			default:
				return model.Fail(model.Unsupported, "mp4", "unrecognized or protected MP4 sample entry")
			}
			if err != nil {
				return err
			}
			variant.Codecs = append(variant.Codecs, codec)
		}
	}
	return nil
}

// mp4Range verifies that partial responses identify exactly the requested byte
// positions and a stable complete length. A server ignoring Range is rejected.
func (p *Pool) mp4Range(
	ctx context.Context,
	candidate model.Candidate,
	start, length, expectedTotal int64,
) (network.Response, int64, error) {
	response, err := p.client.Fetch(ctx, network.Request{
		URL:     candidate.URL,
		Context: candidate.Context,
		Headers: map[string]string{
			"Range": fmt.Sprintf("bytes=%d-%d", start, start+length-1),
		},
		Limit:  length,
		Sample: true,
	})
	if err != nil {
		return network.Response{}, 0, err
	}
	if response.Status != http.StatusPartialContent {
		return network.Response{}, 0, model.Fail(model.Unsupported, "mp4", "partial media response required")
	}
	contentRange := response.Header.Get("Content-Range")
	positions, totalText, found := strings.Cut(strings.TrimPrefix(contentRange, "bytes "), "/")
	startText, endText, hasEnd := strings.Cut(positions, "-")
	actualStart, startErr := strconv.ParseInt(startText, 10, 64)
	actualEnd, endErr := strconv.ParseInt(endText, 10, 64)
	total, totalErr := strconv.ParseInt(totalText, 10, 64)
	if !strings.HasPrefix(contentRange, "bytes ") || !found || !hasEnd ||
		startErr != nil || endErr != nil || totalErr != nil || total <= 0 ||
		actualStart != start || actualEnd < start || actualEnd >= total ||
		actualEnd-start+1 != min(length, total-start) || int64(len(response.Body)) != actualEnd-start+1 ||
		(expectedTotal > 0 && expectedTotal != total) {
		return network.Response{}, 0, model.Fail(model.ProtocolChange, "mp4", "invalid or unstable media byte range")
	}
	return response, total, nil
}

// probeMP4 samples a directly accessible progressive file and reads a bounded
// fast-start moov. Unsupported layouts remain explicit failures; HLS is unchanged.
func (p *Pool) probeMP4(
	ctx context.Context,
	candidate model.Candidate,
	sampleBudget, metadataBudget *byteBudget,
) model.Probed {
	result := model.Probed{
		Source: candidate,
		Observation: model.Observation{
			At:     time.Now(),
			Format: model.MP4,
		},
	}
	fail := func(err error) model.Probed {
		result.Observation.Accessible = false
		result.Observation.DeliveryBPS = nil
		result.Observation.Failure = model.Category(err)
		return result
	}

	firstSize := sampleBudget.reserve(p.limits.SampleBytes)
	if firstSize < 32 {
		return fail(model.Fail(model.Unavailable, "mp4", "sample budget exhausted"))
	}
	first, total, err := p.mp4Range(ctx, candidate, 0, firstSize, 0)
	if err != nil {
		return fail(err)
	}
	if key, ok := evidenceKey(ctx, candidate); ok {
		length := min(int64(len(first.Body)), int64(4096))
		p.prefixes.Put(key, mp4Proof{Digest: sha256.Sum256(first.Body[:length]), Total: total, Length: length}, 15*time.Second, 64)
	}
	if len(first.Body) < 32 || string(first.Body[4:8]) != "ftyp" {
		return fail(model.Fail(model.ProtocolChange, "mp4", "MP4 file header missing"))
	}

	// Only fast-start files with a complete bounded moov following ftyp are mapped.
	fileTypeSize := int64(binary.BigEndian.Uint32(first.Body))
	if fileTypeSize < 16 || fileTypeSize > int64(len(first.Body))-8 || string(first.Body[fileTypeSize+4:fileTypeSize+8]) != "moov" {
		return fail(model.Fail(model.Unsupported, "mp4", "bounded fast-start metadata required"))
	}
	metadataSize := int64(binary.BigEndian.Uint32(first.Body[fileTypeSize:]))
	if metadataSize < 8 || metadataSize > total-fileTypeSize {
		return fail(model.Fail(model.Unsupported, "mp4", "MP4 metadata outside supported bounds"))
	}
	variant := &model.Variant{URL: candidate.URL}
	reader := &mp4MetadataReader{
		pool:      p,
		ctx:       ctx,
		candidate: candidate,
		budget:    metadataBudget,
		initial:   first.Body,
		total:     total,
		pages:     make(map[int64][]byte),
	}
	if err := reader.inspect(fileTypeSize+8, fileTypeSize+metadataSize, variant, 0); err != nil {
		return fail(err)
	}
	if variant.Height == nil || len(variant.Codecs) == 0 {
		return fail(model.Fail(model.Unsupported, "mp4", "clear MP4 track metadata not established"))
	}
	result.Observation.Variant = variant
	result.Observation.ProtectionKnown = true
	result.Observation.FirstByte = &first.FirstByte
	allSpeedsMeasured := true
	measure := func(response network.Response) {
		result.Observation.SamplesCompleted++
		result.Observation.SampleBytes += int64(len(response.Body))
		if response.FirstByte > *result.Observation.FirstByte {
			result.Observation.FirstByte = &response.FirstByte
		}
		if len(response.Body) < 16*1024 || response.Transfer < time.Millisecond {
			allSpeedsMeasured = false
			return
		}
		speed := float64(len(response.Body)-1) * 8 / response.Transfer.Seconds()
		if result.Observation.DeliveryBPS == nil || speed < *result.Observation.DeliveryBPS {
			result.Observation.DeliveryBPS = &speed
		}
	}
	measure(first)
	for sampleIndex := 1; sampleIndex < p.limits.SamplesPerVariant; sampleIndex++ {
		length := sampleBudget.reserve(min(p.limits.SampleBytes, total))
		if length <= 0 {
			return fail(model.Fail(model.Unavailable, "mp4", "sample budget exhausted"))
		}
		// Divide before multiplying to avoid overflow on untrusted resource lengths.
		start := (total - length) / int64(p.limits.SamplesPerVariant-1) * int64(sampleIndex)
		response, _, err := p.mp4Range(ctx, candidate, start, length, total)
		if err != nil {
			return fail(err)
		}
		measure(response)
	}
	if !allSpeedsMeasured {
		result.Observation.DeliveryBPS = nil
	}
	result.Observation.Accessible = true
	return result
}
