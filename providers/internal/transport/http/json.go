package httptransport

import (
	"bytes"
	"encoding/json"
	"io"
	"reflect"
	"strings"
	"time"
)

// uniqueJSON rejects ambiguous duplicate keys as well as excessive nesting.
func uniqueJSON(data []byte) bool {
	decoder := json.NewDecoder(bytes.NewReader(data))
	var value func(int) bool
	value = func(depth int) bool {
		if depth > 32 {
			return false
		}
		token, err := decoder.Token()
		if err != nil {
			return false
		}
		delimiter, compound := token.(json.Delim)
		if !compound {
			return token != nil
		}
		switch delimiter {
		case '{':
			keys := make(map[string]bool)
			for decoder.More() {
				key, err := decoder.Token()
				name, ok := key.(string)
				if err != nil || !ok || name != strings.ToLower(name) || keys[name] {
					return false
				}
				keys[name] = true
				if !value(depth + 1) {
					return false
				}
			}
		case '[':
			for decoder.More() {
				if !value(depth + 1) {
					return false
				}
			}
		default:
			return false
		}
		_, err = decoder.Token()
		return err == nil
	}
	if !value(0) {
		return false
	}
	_, err := decoder.Token()
	return err == io.EOF
}

// responseFits bounds encoder working memory before encoding. It conservatively
// reserves worst-case JSON escaping plus delimiters; the output writer also
// enforces the exact serialized cap. The concrete model has no cyclic fields.
func responseFits(value any) bool {
	remaining := int64(MaxResponseBytes)
	var visit func(reflect.Value) bool
	charge := func(amount int64) bool {
		remaining -= amount
		return remaining >= 0
	}
	visit = func(current reflect.Value) bool {
		if !current.IsValid() {
			return charge(4)
		}
		if current.Type() == reflect.TypeOf(time.Time{}) {
			return charge(40)
		}
		switch current.Kind() {
		case reflect.Pointer, reflect.Interface:
			if current.IsNil() {
				return charge(4)
			}
			return visit(current.Elem())
		case reflect.String:
			return charge(6*int64(current.Len()) + 3)
		case reflect.Map:
			if !charge(2) {
				return false
			}
			iterator := current.MapRange()
			for iterator.Next() {
				if !visit(iterator.Key()) || !visit(iterator.Value()) {
					return false
				}
			}
			return true
		case reflect.Slice, reflect.Array:
			if current.Type().Elem().Kind() == reflect.Uint8 {
				return charge(6*int64(current.Len()) + 3)
			}
			if !charge(2) {
				return false
			}
			for index := 0; index < current.Len(); index++ {
				if !visit(current.Index(index)) {
					return false
				}
			}
			return true
		case reflect.Struct:
			if !charge(2) {
				return false
			}
			for index := 0; index < current.NumField(); index++ {
				field := current.Type().Field(index)
				name := strings.Split(field.Tag.Get("json"), ",")[0]
				if name == "-" || !field.IsExported() {
					continue
				}
				if !charge(6*int64(len(field.Name))+4) || !visit(current.Field(index)) {
					return false
				}
			}
			return true
		default:
			return charge(32)
		}
	}
	return visit(reflect.ValueOf(value))
}
