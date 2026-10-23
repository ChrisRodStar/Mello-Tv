package api

import (
	"net/http"
	"time"
)

func (s *Service) status(w http.ResponseWriter, r *http.Request) error {
	var pendingRevocations, pendingDeletions int
	if e := s.db.QueryRowContext(r.Context(), "SELECT count(*) FROM playbacks WHERE revoked=1 AND revoked_done=0 AND expires>?", time.Now().Unix()).Scan(&pendingRevocations); e != nil {
		return e
	}
	if e := s.db.QueryRowContext(r.Context(), "SELECT count(*) FROM deletions").Scan(&pendingDeletions); e != nil {
		return e
	}
	s.snapMu.RLock()
	revision := s.snapshot.Revision
	exported := s.snapshot.Exported
	count := len(s.snapshot.Records)
	s.snapMu.RUnlock()
	state := "missing"
	if !exported.IsZero() {
		state = "retained"
	}
	write(w, 200, map[string]any{"version": "api.v1", "catalog_snapshot": state, "catalog_revision": revision, "exported_at": exported, "cached_records": count, "pending_revocations": pendingRevocations, "pending_profile_deletions": pendingDeletions})
	return nil
}
