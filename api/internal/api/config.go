package api

import (
	"crypto/ed25519"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"io"
	"net"
	"net/url"
	"os"
)

type Config struct {
	HouseholdAccountID string   `json:"household_account_id,omitempty"`
	TrustedPrivateIPs  []string `json:"trusted_private_ips,omitempty"`
	Listen             string   `json:"listen"`
	DBPath             string   `json:"db_path"`
	Origin             string   `json:"origin"`
	CatalogURL         string   `json:"catalog_url"`
	ProviderURL        string   `json:"provider_url"`
	RelayURL           string   `json:"relay_url"`
	CatalogTokenFile   string   `json:"catalog_token_file"`
	ProviderTokenFile  string   `json:"provider_token_file"`
	RelayTokenFile     string   `json:"relay_token_file"`
	SetupTokenFile     string   `json:"setup_token_file,omitempty"` // Deprecated configuration compatibility; never loaded.
	SigningKeyFile     string   `json:"signing_key_file"`
	EncryptionKeyFile  string   `json:"encryption_key_file"`
	KID                string   `json:"kid"`
	WebDir             string   `json:"web_dir,omitempty"`
}

func LoadConfig(path string) (Config, error) {
	var c Config
	b, e := privateFile(path, 32768)
	if e != nil {
		return c, e
	}
	if e = decode(b, &c); e != nil {
		return c, e
	}
	if c.Listen == "" {
		c.Listen = "127.0.0.1:8090"
	}
	host, port, e := net.SplitHostPort(c.Listen)
	if e != nil || net.ParseIP(host) == nil || !c.trustedIP(net.ParseIP(host)) || port == "0" {
		return c, errors.New("invalid listen")
	}
	if c.DBPath == "" || !safeID(c.KID) {
		return c, errors.New("missing storage or kid")
	}
	for _, v := range []string{c.CatalogURL, c.ProviderURL, c.RelayURL} {
		if e = c.privateURL(v); e != nil {
			return c, e
		}
	}
	seen := map[string]bool{}
	for _, v := range []string{c.CatalogURL, c.ProviderURL, c.RelayURL} {
		u, _ := url.Parse(v)
		if seen[u.Port()] || u.Port() == port {
			return c, errors.New("service ports must be distinct")
		}
		seen[u.Port()] = true
	}
	u, e := url.Parse(c.Origin)
	if e != nil || u.Host == "" || (u.Scheme != "https" && !(u.Scheme == "http" && net.ParseIP(u.Hostname()).IsLoopback())) || u.Path != "" || u.RawQuery != "" || u.Fragment != "" || u.User != nil {
		return c, errors.New("invalid origin")
	}
	return c, nil
}
func privateURL(s string) error { return (Config{}).privateURL(s) }
func (c Config) trustedIP(ip net.IP) bool {
	if ip == nil {
		return false
	}
	if ip.IsLoopback() {
		return true
	}
	if !ip.IsPrivate() {
		return false
	}
	for _, allowed := range c.TrustedPrivateIPs {
		if parsed := net.ParseIP(allowed); parsed != nil && parsed.Equal(ip) {
			return true
		}
	}
	return false
}
func (c Config) privateURL(s string) error {
	u, e := url.Parse(s)
	if e != nil || u.Scheme != "http" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.Path != "" || !c.trustedIP(net.ParseIP(u.Hostname())) || u.Port() == "" {
		return errors.New("private service must use explicitly trusted literal IP and port")
	}
	return nil
}
func privateFile(path string, limit int64) ([]byte, error) {
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	st, e := f.Stat()
	lst, le := os.Lstat(path)
	if e != nil || le != nil || !st.Mode().IsRegular() || lst.Mode()&os.ModeSymlink != 0 || st.Mode().Perm()&0077 != 0 {
		return nil, errors.New("private regular file required")
	}
	b, e := io.ReadAll(io.LimitReader(f, limit+1))
	if int64(len(b)) > limit {
		return nil, errors.New("file too large")
	}
	return b, e
}
func loadSigner(path string) (ed25519.PrivateKey, error) {
	b, e := privateFile(path, 8192)
	if e != nil {
		return nil, e
	}
	if p, _ := pem.Decode(b); p != nil {
		b = p.Bytes
	}
	k, e := x509.ParsePKCS8PrivateKey(b)
	if e != nil {
		return nil, e
	}
	s, ok := k.(ed25519.PrivateKey)
	if !ok {
		return nil, errors.New("Ed25519 required")
	}
	return s, nil
}
