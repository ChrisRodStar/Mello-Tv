package search

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"stv-providers/internal/model"
)

// flight manages in-flight deduplication (singleflight) for concurrent duplicate requests.
type flight struct {
	done                chan struct{}
	ready               chan struct{}
	initial             model.Selection
	published           bool
	completionRequested bool
	cancel              context.CancelFunc
	waiters             int
	abandoned           bool
	result              model.Selection
	err                 error
}

// requestKey derives a deterministic SHA-256 hash identifying the title lookup and context scope.
func requestKey(lookup model.Lookup) (string, error) {
	lookup.WaitForComplete = false
	serialized, err := json.Marshal(struct {
		Lookup model.Lookup `json:"lookup"`
		Scope  string       `json:"scope"`
	}{
		Lookup: lookup,
		Scope:  lookup.ContextKey,
	})
	if err != nil {
		return "", err
	}

	hash := sha256.Sum256(serialized)
	return hex.EncodeToString(hash[:]), nil
}

// cloneSelection creates an isolated deep copy of a Selection result.
// Uses native deep cloning instead of JSON marshal/unmarshal for maximum performance.
func cloneSelection(sel model.Selection) model.Selection {
	return sel.Clone()
}

// cloneLookup creates an isolated deep copy of a Lookup struct.
// Uses native deep cloning instead of JSON marshal/unmarshal for maximum performance.
func cloneLookup(lookup model.Lookup) model.Lookup {
	return lookup.Clone()
}

func sourceKey(lookup model.Lookup) (string, error) {
	lookup.Refresh = false
	lookup.StartupHeight = 0
	return requestKey(lookup)
}
