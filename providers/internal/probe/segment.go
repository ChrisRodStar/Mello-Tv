package probe

// IsTransportStream recognizes aligned MPEG-TS packet headers independently of
// the HTTP MIME label. Require five complete 188-byte packets, sync bytes, clear
// transport-error indicators and nonreserved adaptation/payload control fields.
// This is container evidence for bounded probing, not decoded video/audio proof.
func IsTransportStream(body []byte) bool {
	const packetSize = 188
	const requiredPackets = 5
	if len(body) < requiredPackets*packetSize {
		return false
	}
	for packet := 0; packet < requiredPackets; packet++ {
		header := body[packet*packetSize:]
		if header[0] != 0x47 || header[1]&0x80 != 0 || header[3]&0x30 == 0 {
			return false
		}
		// An adaptation field must fit inside its packet. Payload-only packets
		// have no adaptation length byte to inspect.
		if header[3]&0x20 != 0 && int(header[4])+5 > packetSize {
			return false
		}
	}
	return true
}
