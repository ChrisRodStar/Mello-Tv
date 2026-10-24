package network

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
)

// deniedPrefixes lists private, loopback, link-local, multicast, documentation, and CGNAT IP prefixes.
var deniedPrefixes = []netip.Prefix{
	// IPv4 Private and Special-Use
	netip.MustParsePrefix("0.0.0.0/8"),       // RFC 1122 Current network (only valid as source)
	netip.MustParsePrefix("10.0.0.0/8"),      // RFC 1918 Private IPv4
	netip.MustParsePrefix("100.64.0.0/10"),   // RFC 6598 Shared Address Space (Carrier-Grade NAT)
	netip.MustParsePrefix("127.0.0.0/8"),     // RFC 1122 Loopback
	netip.MustParsePrefix("169.254.0.0/16"),  // RFC 3927 Link-Local
	netip.MustParsePrefix("172.16.0.0/12"),   // RFC 1918 Private IPv4
	netip.MustParsePrefix("192.0.0.0/24"),    // RFC 6890 IETF Protocol Assignments
	netip.MustParsePrefix("192.0.2.0/24"),    // RFC 5737 Documentation (TEST-NET-1)
	netip.MustParsePrefix("192.168.0.0/16"),  // RFC 1918 Private IPv4
	netip.MustParsePrefix("198.18.0.0/15"),   // RFC 2544 Benchmarking
	netip.MustParsePrefix("198.51.100.0/24"), // RFC 5737 Documentation (TEST-NET-2)
	netip.MustParsePrefix("203.0.113.0/24"),  // RFC 5737 Documentation (TEST-NET-3)
	netip.MustParsePrefix("224.0.0.0/4"),     // RFC 5771 Multicast
	netip.MustParsePrefix("240.0.0.0/4"),     // RFC 1112 Reserved for Future Use
	// IPv6 Special-Use and Unique Local
	netip.MustParsePrefix("::/96"),          // IPv4-compatible IPv6 (deprecated)
	netip.MustParsePrefix("::ffff:0:0/96"),  // IPv4-mapped IPv6
	netip.MustParsePrefix("64:ff9b::/96"),   // RFC 6052 Well-Known IPv4/IPv6 Prefix
	netip.MustParsePrefix("64:ff9b:1::/48"), // Local-Use IPv4/IPv6 Translation
	netip.MustParsePrefix("100::/64"),       // RFC 6666 Discard-Only Prefix
	netip.MustParsePrefix("2001::/23"),      // RFC 2928 IETF Protocol Assignments
	netip.MustParsePrefix("2001:db8::/32"),  // RFC 3849 Documentation
	netip.MustParsePrefix("3fff::/20"),      // Documentation
	netip.MustParsePrefix("2002::/16"),      // RFC 3056 6to4 Anycast
	netip.MustParsePrefix("fc00::/7"),       // RFC 4193 Unique Local Address (ULA)
	netip.MustParsePrefix("fe80::/10"),      // RFC 4291 Link-Local Unicast
	netip.MustParsePrefix("ff00::/8"),       // RFC 4291 Multicast
}

var ipv6GlobalUnicast = netip.MustParsePrefix("2000::/3")

// PublicIP verifies that an IP address is a valid, globally routable public unicast address.
func PublicIP(ip netip.Addr) bool {
	if !ip.IsValid() {
		return false
	}
	ip = ip.Unmap()

	// Enforce IPv6 global unicast prefix 2000::/3.
	if ip.Is6() && !ipv6GlobalUnicast.Contains(ip) {
		return false
	}

	if !ip.IsGlobalUnicast() || ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
		return false
	}

	for _, prefix := range deniedPrefixes {
		if prefix.Contains(ip) {
			return false
		}
	}

	return true
}

// ValidateURL performs strict destination validation to guard against SSRF and local network traversal.
func ValidateURL(u *url.URL) error {
	if u == nil || (u.Scheme != "https" && u.Scheme != "http") || u.Hostname() == "" || u.User != nil || u.Opaque != "" || u.Fragment != "" {
		return errors.New("invalid destination")
	}

	hostname := strings.ToLower(strings.TrimSuffix(u.Hostname(), "."))
	if hostname == "" || hostname == "localhost" || strings.HasSuffix(hostname, ".localhost") || strings.HasSuffix(hostname, ".local") || strings.Contains(hostname, "%") || strings.ContainsAny(hostname, " \t\r\n") {
		return errors.New("local destination denied")
	}

	// Prohibit raw numeric and hex destination formats used in SSRF evasion.
	if _, err := strconv.ParseUint(hostname, 10, 64); err == nil {
		return errors.New("numeric destination denied")
	}

	if strings.ContainsRune(hostname, '.') {
		numDots := 0
		hasAmbiguous := false
		allNumeric := true
		partStart := 0
		for i := 0; i <= len(hostname); i++ {
			if i == len(hostname) || hostname[i] == '.' {
				part := hostname[partStart:i]
				partStart = i + 1
				numDots++
				if len(part) == 0 {
					allNumeric = false
					break
				}
				if strings.HasPrefix(part, "0x") || strings.HasPrefix(part, "0X") {
					return errors.New("ambiguous destination denied")
				}
				for j := 0; j < len(part); j++ {
					if part[j] < '0' || part[j] > '9' {
						allNumeric = false
						break
					}
				}
				if len(part) > 1 && part[0] == '0' {
					hasAmbiguous = true
				}
			}
		}
		if numDots == 4 && allNumeric && hasAmbiguous {
			return errors.New("ambiguous destination denied")
		}
	}

	port := u.Port()
	if port != "" && port != "80" && port != "443" {
		return errors.New("destination port denied")
	}

	if ip, err := netip.ParseAddr(hostname); err == nil && !PublicIP(ip) {
		return errors.New("nonpublic destination denied")
	}

	return nil
}

// Origin returns the canonical scheme://hostname:port origin representation for a URL.
func Origin(u *url.URL) string {
	if u == nil {
		return ""
	}
	port := u.Port()
	if port == "" {
		if u.Scheme == "https" {
			port = "443"
		} else {
			port = "80"
		}
	}
	return u.Scheme + "://" + net.JoinHostPort(strings.ToLower(u.Hostname()), port)
}

// scopeOrigin returns the normalized origin string if the URL passes destination validation.
func scopeOrigin(rawURL string) string {
	parsedURL, err := url.Parse(rawURL)
	if err != nil || ValidateURL(parsedURL) != nil {
		return ""
	}
	return Origin(parsedURL)
}

// dial performs guarded DNS resolution and establishes an outbound connection to a public IP.
//
// Pipeline:
//  1. Check VPN guard.
//  2. Resolve all IP answers via DoH or tunnel resolver.
//  3. Enforce that every IP in the DNS response is a valid public IP (preventing DNS rebinding).
//  4. Dial literal IP directly to prevent second-resolution TOCTOU, preserving original host for TLS SNI.
func (c *Client) dial(ctx context.Context, networkProto, address string) (net.Conn, error) {
	if c == nil || c.guard == nil {
		return nil, errors.New("VPN route unavailable")
	}
	if err := c.guard.Check(ctx); err != nil {
		return nil, errors.New("VPN route unavailable")
	}

	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return nil, errors.New("invalid dial address")
	}

	if _, err := strconv.Atoi(port); err != nil || (port != "80" && port != "443") {
		return nil, errors.New("port denied")
	}

	resolver := c.resolver
	if resolver == nil {
		resolver = net.DefaultResolver
	}

	ips, err := resolver.LookupNetIP(ctx, c.family, host)
	if err != nil || len(ips) == 0 {
		return nil, errors.New("destination DNS unavailable")
	}

	// Validate all DNS answers before dialing any.
	for _, ip := range ips {
		if !PublicIP(ip) {
			return nil, errors.New("nonpublic DNS answer denied")
		}
	}

	// Attempt connection to resolved IPs in order.
	for _, ip := range ips {
		if err := c.guard.Check(ctx); err != nil {
			return nil, errors.New("VPN route unavailable")
		}

		conn, dialErr := c.dialer.DialContext(ctx, networkProto, net.JoinHostPort(ip.String(), port))
		if dialErr == nil {
			return conn, nil
		}

		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
	}

	if ctx.Err() != nil {
		return nil, ctx.Err()
	}

	return nil, errors.New("destination connection failed")
}
