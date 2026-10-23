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

// deniedCIDRs lists private, loopback, link-local, multicast, documentation, and CGNAT IP prefixes.
var deniedCIDRs = []string{
	// IPv4 Private & Special-Use
	"0.0.0.0/8",       // RFC 1122 Current network (only valid as source)
	"10.0.0.0/8",      // RFC 1918 Private IPv4
	"100.64.0.0/10",   // RFC 6598 Shared Address Space (Carrier-Grade NAT)
	"127.0.0.0/8",     // RFC 1122 Loopback
	"169.254.0.0/16",  // RFC 3927 Link-Local
	"172.16.0.0/12",   // RFC 1918 Private IPv4
	"192.0.0.0/24",    // RFC 6890 IETF Protocol Assignments
	"192.0.2.0/24",    // RFC 5737 Documentation (TEST-NET-1)
	"192.168.0.0/16",  // RFC 1918 Private IPv4
	"198.18.0.0/15",   // RFC 2544 Benchmarking
	"198.51.100.0/24", // RFC 5737 Documentation (TEST-NET-2)
	"203.0.113.0/24",  // RFC 5737 Documentation (TEST-NET-3)
	"224.0.0.0/4",     // RFC 5771 Multicast
	"240.0.0.0/4",     // RFC 1112 Reserved for Future Use
	// IPv6 Special-Use & Unique Local
	"::/96",          // IPv4-compatible IPv6 (deprecated)
	"::ffff:0:0/96",  // IPv4-mapped IPv6
	"64:ff9b::/96",   // RFC 6052 Well-Known IPv4/IPv6 Prefix
	"64:ff9b:1::/48", // Local-Use IPv4/IPv6 Translation
	"100::/64",       // RFC 6666 Discard-Only Prefix
	"2001::/23",      // RFC 2928 IETF Protocol Assignments
	"2001:db8::/32",  // RFC 3849 Documentation
	"3fff::/20",      // Documentation
	"2002::/16",      // RFC 3056 6to4 Anycast
	"fc00::/7",       // RFC 4193 Unique Local Address (ULA)
	"fe80::/10",      // RFC 4291 Link-Local Unicast
	"ff00::/8",       // RFC 4291 Multicast
}

// PublicIP verifies that an IP address is a globally routable public unicast address.
func PublicIP(ip netip.Addr) bool {
	ip = ip.Unmap()

	// Enforce IPv6 global unicast prefix 2000::/3.
	if ip.Is6() && !netip.MustParsePrefix("2000::/3").Contains(ip) {
		return false
	}

	if !ip.IsGlobalUnicast() || ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
		return false
	}

	for _, cidr := range deniedCIDRs {
		if netip.MustParsePrefix(cidr).Contains(ip) {
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
	if hostname == "localhost" || strings.HasSuffix(hostname, ".localhost") || strings.HasSuffix(hostname, ".local") || strings.Contains(hostname, "%") {
		return errors.New("local destination denied")
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
//  2. Resolve all IP answers via DoH/tunnel resolver.
//  3. Enforce that every IP in the DNS response is a valid public IP (preventing DNS rebinding).
//  4. Dial literal IP directly to prevent second-resolution TOCTOU, preserving original host for TLS SNI.
func (c *Client) dial(ctx context.Context, networkProto, address string) (net.Conn, error) {
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

	ips, err := c.resolver.LookupNetIP(ctx, c.family, host)
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

	return nil, errors.New("destination connection failed")
}
