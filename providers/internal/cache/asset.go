package cache

import (
	"net/http"
	"strconv"
	"strings"
	"time"
)

// AssetLifetime bounds reuse of validated static protocol material. Responses
// that install session state or require revalidation must be fetched again.
func AssetLifetime(header http.Header, maximum time.Duration) time.Duration {
	if maximum <= 0 || len(header.Values("Set-Cookie")) != 0 {
		return 0
	}

	lifetime := maximum
	for _, raw := range strings.Split(strings.Join(header.Values("Cache-Control"), ","), ",") {
		raw = strings.TrimSpace(raw)
		if raw == "" {
			continue
		}
		directive := strings.SplitN(strings.ToLower(raw), "=", 2)
		switch directive[0] {
		case "no-store", "no-cache", "must-revalidate":
			return 0
		case "max-age":
			if len(directive) != 2 {
				return 0
			}
			seconds, err := strconv.ParseInt(strings.Trim(directive[1], `"`), 10, 64)
			if err != nil || seconds <= 0 {
				return 0
			}
			if seconds < int64(lifetime/time.Second) {
				lifetime = time.Duration(seconds) * time.Second
			}
		}
	}
	return lifetime
}
