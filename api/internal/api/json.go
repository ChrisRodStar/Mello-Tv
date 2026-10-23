package api

import (
	"bytes"
	"encoding/json"
	"io"
	"strings"
)

// Bound nesting and reject duplicate/case-alias object keys before decoding.
func checkJSON(b []byte) error {
	d := json.NewDecoder(bytes.NewReader(b))
	d.UseNumber()
	var value func(int) error
	value = func(depth int) error {
		if depth > 32 {
			return bad()
		}
		t, e := d.Token()
		if e != nil {
			return bad()
		}
		delim, ok := t.(json.Delim)
		if !ok {
			return nil
		}
		switch delim {
		case '{':
			seen := map[string]bool{}
			for d.More() {
				k, e := d.Token()
				if e != nil {
					return bad()
				}
				s, ok := k.(string)
				if !ok {
					return bad()
				}
				fold := strings.ToLower(s)
				if seen[fold] {
					return bad()
				}
				seen[fold] = true
				if e = value(depth + 1); e != nil {
					return e
				}
			}
		case '[':
			for d.More() {
				if e = value(depth + 1); e != nil {
					return e
				}
			}
		default:
			return bad()
		}
		_, e = d.Token()
		return e
	}
	if e := value(0); e != nil {
		return e
	}
	if _, e := d.Token(); e != io.EOF {
		return bad()
	}
	return nil
}
