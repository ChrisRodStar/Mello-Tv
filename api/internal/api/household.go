package api

import (
	"context"
	"database/sql"
	"errors"
	"net/http"
)

// selectHousehold preserves the existing storage owner. Network access is the
// household access boundary; profile IDs separate viewing preferences, not users.
// Multiple legacy owners require an explicit choice rather than data exposure.
func (s *Service) selectHousehold(configured string) (string, error) {
	var household string
	err := transaction(context.Background(), s.db, func(tx *sql.Tx) error {
		household = configured
		if household == "" {
			err := tx.QueryRow("SELECT value FROM settings WHERE key='household-account-id'").Scan(&household)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				return err
			}
		}
		if household == "" {
			rows, err := tx.Query("SELECT id FROM accounts ORDER BY id LIMIT 2")
			if err != nil {
				return err
			}
			var owners []string
			for rows.Next() {
				var owner string
				if err := rows.Scan(&owner); err != nil {
					rows.Close()
					return err
				}
				owners = append(owners, owner)
			}
			rowErr := rows.Err()
			rows.Close()
			if rowErr != nil {
				return rowErr
			}
			switch len(owners) {
			case 0:
				household = id()
				// Retain schema compatibility without creating usable credentials.
				if _, err := tx.Exec("INSERT INTO accounts VALUES(?,?,?,?,?)", household, "household", []byte{}, []byte{}, true); err != nil {
					return err
				}
			case 1:
				household = owners[0]
			default:
				return errors.New("multiple household owners require household_account_id")
			}
		}
		var count int
		if err := tx.QueryRow("SELECT count(*) FROM accounts WHERE id=?", household).Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return errors.New("household owner unavailable")
		}
		_, err := tx.Exec("INSERT INTO settings(key,value) VALUES('household-account-id',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", household)
		return err
	})
	return household, err
}

func (s *Service) profile(r *http.Request, account, pid string) (Profile, error) {
	var p Profile
	var b []byte
	e := s.db.QueryRowContext(r.Context(), "SELECT data FROM profiles WHERE id=? AND account=?", pid, account).Scan(&b)
	if errors.Is(e, sql.ErrNoRows) {
		return p, missing()
	}
	if e != nil {
		return p, e
	}
	e = decode(b, &p)
	return p, e
}
