package model

import (
	"context"
	"errors"
	"slices"
	"time"
)

// ErrorCategory categorizes failures without exposing sensitive upstream URLs or headers.
type ErrorCategory string

const (
	Unavailable     ErrorCategory = "unavailable"
	Unsupported     ErrorCategory = "unsupported"
	Timeout         ErrorCategory = "timeout"
	Cancelled       ErrorCategory = "cancellation"
	UpstreamFailure ErrorCategory = "upstream_failure"
	ProtocolChange  ErrorCategory = "protocol_change"
)

// Failure represents a controlled error classification, pipeline stage, and explanation.
// It implements the standard error interface, Unwrap for error chains, and Is for matching.
type Failure struct {
	Err      error         `json:"-"`
	Category ErrorCategory `json:"category"`
	Stage    string        `json:"stage"`
	Detail   string        `json:"detail"`
}

// Error formats the failure into a human-readable string.
func (f *Failure) Error() string {
	base := string(f.Category) + ": " + f.Stage + ": " + f.Detail
	if f.Err != nil {
		return base + ": " + f.Err.Error()
	}
	return base
}

// Unwrap returns the underlying wrapped error, enabling errors.Is and errors.As traversal.
func (f *Failure) Unwrap() error {
	return f.Err
}

// Is reports whether target matches this failure by category or error type.
func (f *Failure) Is(target error) bool {
	if f == nil {
		return false
	}
	var other *Failure
	if errors.As(target, &other) {
		if other.Category != "" && f.Category != other.Category {
			return false
		}
		if other.Stage != "" && f.Stage != other.Stage {
			return false
		}
		if other.Detail != "" && f.Detail != other.Detail {
			return false
		}
		return true
	}
	return false
}

// Fail creates a new Failure instance as a standard Go error.
func Fail(category ErrorCategory, stage, detail string) error {
	return &Failure{
		Category: category,
		Stage:    stage,
		Detail:   detail,
	}
}

// Wrap creates a new Failure instance wrapping an existing underlying error.
func Wrap(category ErrorCategory, stage, detail string, err error) error {
	return &Failure{
		Category: category,
		Stage:    stage,
		Detail:   detail,
		Err:      err,
	}
}

// Category extracts the broad ErrorCategory from an arbitrary error.
// It traverses wrapped errors and recognizes standard context errors.
func Category(err error) ErrorCategory {
	if err == nil {
		return ""
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return Timeout
	}
	if errors.Is(err, context.Canceled) {
		return Cancelled
	}

	var failure *Failure
	if errors.As(err, &failure) {
		return failure.Category
	}

	return UpstreamFailure
}

// AdapterResult contains discovered candidates and non-fatal issues from a single provider.
type AdapterResult struct {
	Issues     []Failure     `json:"issues,omitempty"`
	Candidates []Candidate   `json:"candidates"`
	Duration   time.Duration `json:"resolver_duration"`
}

// ProviderStatus summarizes the execution outcome for one provider during a coordinated search.
type ProviderStatus struct {
	Provider       ProviderID    `json:"provider"`
	Failure        ErrorCategory `json:"failure,omitempty"`
	Duration       time.Duration `json:"duration"`
	CandidateCount int           `json:"candidate_count"`
}

// Ranked pairs a probed candidate with its within-tier score and selection rationale.
// Score does not override language, established-delivery, or resolution ordering.
type Ranked struct {
	Candidate Probed   `json:"candidate"`
	Reasons   []string `json:"reasons"`
	Score     float64  `json:"score"`
}

// Clone returns an isolated deep copy of Ranked.
func (r Ranked) Clone() Ranked {
	cloned := r
	cloned.Candidate = r.Candidate.Clone()
	cloned.Reasons = slices.Clone(r.Reasons)
	return cloned
}

// ContractVersion defines the private contract schema version for consumer integration.
const ContractVersion = "providers.v1"

// Selection contains the final winning stream, ordered backups, and rejected candidates.
type Selection struct {
	Rejected  map[string]string `json:"rejected"`
	Selected  *Ranked           `json:"selected,omitempty"`
	Version   string            `json:"version"`
	Backups   []Ranked          `json:"backups"`
	Providers []ProviderStatus  `json:"providers"`
	Pending   bool              `json:"pending,omitempty"`
}

// Clone returns an isolated deep copy of Selection with independent maps and slices.
// Eliminates expensive JSON marshal/unmarshal cycles during concurrent flight deduplication.
func (s Selection) Clone() Selection {
	cloned := s
	if s.Selected != nil {
		sel := s.Selected.Clone()
		cloned.Selected = &sel
	}
	if s.Backups != nil {
		cloned.Backups = make([]Ranked, len(s.Backups))
		for i, b := range s.Backups {
			cloned.Backups[i] = b.Clone()
		}
	}
	if s.Rejected != nil {
		cloned.Rejected = make(map[string]string, len(s.Rejected))
		for k, v := range s.Rejected {
			cloned.Rejected[k] = v
		}
	}
	cloned.Providers = slices.Clone(s.Providers)
	return cloned
}
