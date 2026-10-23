package probe

import (
	"encoding/binary"
	"errors"
	"fmt"
)

// mp4SampleCodec derives RFC 6381 AVC and MPEG-4 audio names from the sample
// entry's configuration, never from a provider label. Missing configuration
// retains only the known sample-entry name; it cannot satisfy a specific profile.
func mp4SampleCodec(kind string, configuration []byte) (string, error) {
	boxes, err := mp4Boxes(configuration)
	if err != nil {
		return "", err
	}

	codec := kind
	found := false
	for _, box := range boxes {
		if (kind == "avc1" || kind == "avc3") && box.Kind == "avcC" {
			if found || len(box.Data) < 7 || box.Data[0] != 1 {
				return "", errors.New("invalid or duplicate AVC configuration")
			}
			codec = fmt.Sprintf("%s.%02x%02x%02x", kind, box.Data[1], box.Data[2], box.Data[3])
			found = true
		}

		if kind == "mp4a" && box.Kind == "esds" {
			if found {
				return "", errors.New("duplicate MPEG-4 audio configuration")
			}
			codec, err = mp4AudioCodec(box.Data)
			if err != nil {
				return "", err
			}
			found = true
		}
	}

	return codec, nil
}

// MPEG-4 descriptor sizes use at most four base-128 bytes. Return a bounded
// payload and its siblings, rather than scanning arbitrary bytes for tag values.
func mp4Descriptor(body []byte) (byte, []byte, []byte, error) {
	if len(body) < 2 {
		return 0, nil, nil, errors.New("truncated MPEG-4 descriptor")
	}
	tag := body[0]
	length := 0
	for index := 1; index <= 4 && index < len(body); index++ {
		length = length<<7 | int(body[index]&0x7f)
		if body[index]&0x80 == 0 {
			headerSize := index + 1
			if length > len(body)-headerSize {
				return 0, nil, nil, errors.New("MPEG-4 descriptor outside bounds")
			}
			return tag, body[headerSize : headerSize+length], body[headerSize+length:], nil
		}
	}

	return 0, nil, nil, errors.New("invalid MPEG-4 descriptor size")
}

func mp4AudioCodec(body []byte) (string, error) {
	if len(body) < 4 || binary.BigEndian.Uint32(body) != 0 {
		return "", errors.New("unsupported MPEG-4 descriptor version")
	}
	tag, elementaryStream, remaining, err := mp4Descriptor(body[4:])
	if err != nil || tag != 3 || len(remaining) != 0 || len(elementaryStream) < 3 {
		return "", errors.New("invalid elementary stream descriptor")
	}

	flags := elementaryStream[2]
	offset := 3
	if flags&0x80 != 0 {
		offset += 2 // dependsOn_ES_ID
	}
	if flags&0x40 != 0 {
		if offset >= len(elementaryStream) {
			return "", errors.New("truncated elementary stream URL")
		}
		offset += 1 + int(elementaryStream[offset])
	}
	if flags&0x20 != 0 {
		offset += 2 // OCR_ES_Id
	}
	if offset > len(elementaryStream) {
		return "", errors.New("elementary stream flags outside bounds")
	}

	tag, decoder, _, err := mp4Descriptor(elementaryStream[offset:])
	if err != nil || tag != 4 || len(decoder) < 13 || (decoder[1]>>2)&0x3f != 5 {
		return "", errors.New("invalid audio decoder configuration")
	}
	objectType := decoder[0]
	if objectType != 0x40 {
		return fmt.Sprintf("mp4a.%02x", objectType), nil
	}

	// RFC 6381 adds the decimal AudioObjectType from AudioSpecificConfig for
	// MPEG-4 Audio (OTI 0x40). Value 31 escapes to a six-bit extension plus 32.
	for descriptors := decoder[13:]; len(descriptors) > 0; {
		tag, specific, rest, err := mp4Descriptor(descriptors)
		if err != nil {
			return "", err
		}
		descriptors = rest
		if tag != 5 {
			continue
		}
		if len(specific) < 2 {
			return "", errors.New("truncated audio specific configuration")
		}
		audioType := int(specific[0] >> 3)
		if audioType == 31 {
			audioType = 32 + int(binary.BigEndian.Uint16(specific)>>5&0x3f)
		}
		if audioType == 0 {
			return "", errors.New("invalid audio object type")
		}
		return fmt.Sprintf("mp4a.40.%d", audioType), nil
	}

	// The OTI is known, but no audio object type was established.
	return "mp4a.40", nil
}
