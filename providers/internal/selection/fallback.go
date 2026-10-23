package selection

import (
	"family-stream-providers/internal/model"
	"sort"
)

// Select evaluates all probed candidates, eliminates ineligible streams, scores the remainder,
// and returns the best stream along with ordered backups and rejection reasons.
func Select(lookup model.Lookup, items []model.Probed) model.Selection {
	result := model.Selection{
		Version:  model.ContractVersion,
		Backups:  []model.Ranked{},
		Rejected: make(map[string]string),
	}

	var rankedCandidates []model.Ranked
	for _, item := range items {
		isEligible, rejectionReason := Eligible(lookup, item)
		if !isEligible {
			result.Rejected[item.Source.ID] = rejectionReason
			continue
		}

		rankedCandidates = append(rankedCandidates, rank(lookup, item))
	}

	for index := range rankedCandidates {
		if hasVerifiedAdaptiveLower(rankedCandidates[index].Candidate, rankedCandidates) {
			rankedCandidates[index].Reasons = append(rankedCandidates[index].Reasons, adaptiveReason)
		}
	}

	// Preferences and viable resolution take precedence over the within-tier score.
	sort.SliceStable(rankedCandidates, func(firstIndex, secondIndex int) bool {
		return prefer(lookup, rankedCandidates[firstIndex], rankedCandidates[secondIndex])
	})

	if len(rankedCandidates) > 0 {
		result.Selected = &rankedCandidates[0]
		result.Backups = append(result.Backups, rankedCandidates[1:]...)
	}

	return result
}
