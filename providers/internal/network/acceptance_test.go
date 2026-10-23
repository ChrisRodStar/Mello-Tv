package network

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func newAllowance(t *testing.T, bytes int64, requests int) string {
	t.Helper()
	directory := t.TempDir()
	if os.Chmod(directory, 0700) != nil {
		t.Fatal("private directory mode")
	}
	path := filepath.Join(directory, "go-allowance.jsonl")
	data, _ := json.Marshal(allowance{
		RemainingBytes:    bytes,
		RemainingRequests: requests,
		DeadlineMS:        time.Now().Add(time.Minute).UnixMilli(),
	})
	if os.WriteFile(path, append(data, '\n'), 0600) != nil {
		t.Fatal("create synthetic allowance")
	}
	return path
}
func TestAcceptanceAllowancePersistsAcrossExclusiveOwners(t *testing.T) {
	path := newAllowance(t, 10000, 20)
	budget, err := OpenAcceptanceBudget(path)
	if err != nil {
		t.Fatal(err)
	}
	if duplicate, err := OpenAcceptanceBudget(path); err == nil {
		duplicate.Close()
		t.Fatal("duplicate owner admitted")
	}
	var workers sync.WaitGroup
	for index := 0; index < 8; index++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			if budget.Take(1000) != nil {
				t.Error("reservation rejected")
			}
		}()
	}
	workers.Wait()
	deadline := budget.state.DeadlineMS
	budget.Close()
	reopened, err := OpenAcceptanceBudget(path)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if reopened.state.RemainingBytes != 2000 || reopened.state.RemainingRequests != 12 || reopened.state.DeadlineMS != deadline {
		t.Fatal("allowance reset on restart")
	}
	if reopened.Take(2001) == nil || reopened.Take(1) == nil || reopened.Check() == nil {
		t.Fatal("stopped run continued")
	}
}
func TestAcceptanceAllowanceRejectsIncompleteAndExpiredState(t *testing.T) {
	for _, data := range []string{
		`{"remaining_bytes":1,"remaining_requests":1,"deadline_ms":1}` + "\n",

		`{"remaining_bytes":1,"remaining_requests":1,"deadline_ms":99999999999999}`,

		"not json\n",
	} {
		path := filepath.Join(t.TempDir(), "allowance")
		os.WriteFile(path, []byte(data), 0600)
		if budget, err := OpenAcceptanceBudget(path); err == nil {
			budget.Close()
			t.Fatal("invalid allowance opened")
		}
	}
}

func TestFinalPrepaidRequestSurvivesCreditExhaustion(t *testing.T) {
	path := newAllowance(t, 1_000_000, 1)
	budget, err := OpenAcceptanceBudget(path)
	if err != nil {
		t.Fatal(err)
	}
	defer budget.Close()
	client, err := New(Options{
		Guard: GuardFunc(func(context.Context) error {
			return budget.WorkCheck()
		}),
		Budget: budget,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	called := false
	client.http.Transport = fakeTransport(func(*http.Request) (*http.Response, error) {
		called = true
		if budget.WorkCheck() != nil {
			t.Fatal("last prepaid request was rejected as VPN unavailable")
		}
		return &http.Response{
			StatusCode: 200,
			Header:     make(http.Header),
			Body:       io.NopCloser(strings.NewReader("synthetic")),
		}, nil
	})
	response, err := client.Fetch(context.Background(), Request{
		URL:   "https://media.example.com/manifest",
		Limit: 1024,
	})
	if err != nil || !called || string(response.Body) != "synthetic" {
		t.Fatal("prepaid fetch did not complete")
	}
	if budget.Check() == nil || budget.Take(1) == nil {
		t.Fatal("new spending allowed after exhaustion")
	}
	if budget.WorkCheck() != nil {
		t.Fatal("credit stop invalidated prepaid work")
	}
}
