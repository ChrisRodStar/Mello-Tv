package cache

import (
	"sync"
	"time"
)

// Values retains a small set of already validated, immutable protocol values.
// It never stores session tokens or performs network work while holding a lock.
// The zero value is usable; callers supply fixed positive bounds on insertion.
type Values[Value any] struct {
	mutex   sync.Mutex
	entries map[string]valueEntry[Value]
}

type valueEntry[Value any] struct {
	value   Value
	expires time.Time
}

func (values *Values[Value]) Get(key string) (Value, bool) {
	values.mutex.Lock()
	defer values.mutex.Unlock()
	entry, found := values.entries[key]
	if !found || !time.Now().Before(entry.expires) {
		delete(values.entries, key)
		var empty Value
		return empty, false
	}
	return entry.value, true
}

func (values *Values[Value]) Put(key string, value Value, lifetime time.Duration, capacity int) {
	if capacity <= 0 || lifetime <= 0 {
		return
	}
	values.mutex.Lock()
	defer values.mutex.Unlock()
	if values.entries == nil {
		values.entries = make(map[string]valueEntry[Value])
	}
	now := time.Now()
	for entryKey, entry := range values.entries {
		if !now.Before(entry.expires) {
			delete(values.entries, entryKey)
		}
	}
	if _, replacing := values.entries[key]; !replacing && len(values.entries) >= capacity {
		for entryKey := range values.entries {
			delete(values.entries, entryKey)
			break
		}
	}
	values.entries[key] = valueEntry[Value]{value: value, expires: now.Add(lifetime)}
}

// Delete invalidates a value after an access failure or explicit refresh.
func (values *Values[Value]) Delete(key string) {
	values.mutex.Lock()
	delete(values.entries, key)
	values.mutex.Unlock()
}
