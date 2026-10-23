package api

import (
	"encoding/json"
	"reflect"
)

func requestShape(b []byte, t reflect.Type) error {
	for t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	if t.Kind() != reflect.Struct {
		return nil
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(b, &object) != nil || object == nil {
		return bad()
	}
	fields := map[string]reflect.Type{}
	var collect func(reflect.Type)
	collect = func(t reflect.Type) {
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			tag := f.Tag.Get("json")
			if f.Anonymous && tag == "" {
				collect(f.Type)
				continue
			}
			name := tag
			for j, c := range name {
				if c == ',' {
					name = name[:j]
					break
				}
			}
			if name != "" && name != "-" {
				fields[name] = f.Type
			}
		}
	}
	collect(t)
	for key, raw := range object {
		ft, ok := fields[key]
		if !ok {
			return bad()
		}
		if ft.Kind() == reflect.Pointer && string(raw) == "null" {
			continue
		}
		for ft.Kind() == reflect.Pointer {
			ft = ft.Elem()
		}
		if ft.Kind() == reflect.Struct {
			if e := requestShape(raw, ft); e != nil {
				return e
			}
		}
	}
	return nil
}
func requiredKeys(b []byte, keys ...string) bool {
	var m map[string]json.RawMessage
	if json.Unmarshal(b, &m) != nil {
		return false
	}
	for _, k := range keys {
		v, ok := m[k]
		if !ok || string(v) == "null" {
			return false
		}
	}
	return true
}
