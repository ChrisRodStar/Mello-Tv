package model

import (
	"context"
	"errors"
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
type Failure struct {
	Category ErrorCategory `json:"category"`
	Stage    string        `json:"stage"`
	Detail   string        `json:"detail"`
}

// Error formats the failure into a human-readable string.
func (f *Failure) Error() string {
	return string(f.Category) + ": " + f.Stage + ": " + f.Detail
}

// Fail creates a new Failure instance as a standard Go error.
func Fail(category ErrorCategory, stage, detail string) error {
	return &Failure{
		Category: category,
		Stage:    stage,
		Detail:   detail,
	}
}

// Category extracts the broad ErrorCategory from an arbitrary error.
func Category(err error) ErrorCategory {
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
	Duration       time.Duration `json:"duration"`
	CandidateCount int           `json:"candidate_count"`
	Failure        ErrorCategory `json:"failure,omitempty"`
}

// Ranked pairs a probed candidate with its within-tier score and selection rationale.
// Score does not override language, established-delivery, or resolution ordering.
type Ranked struct {
	Candidate Probed   `json:"candidate"`
	Score     float64  `json:"score"`
	Reasons   []string `json:"reasons"`
}

// ContractVersion defines the private contract schema version for consumer integration.
const ContractVersion = "providers.v1"

// Selection contains the final winning stream, ordered backups, and rejected candidates.
type Selection struct {
	Version   string            `json:"version"`
	Pending   bool              `json:"pending,omitempty"`
	Selected  *Ranked           `json:"selected,omitempty"`
	Backups   []Ranked          `json:"backups"`
	Rejected  map[string]string `json:"rejected"`
	Providers []ProviderStatus  `json:"providers"`
}
