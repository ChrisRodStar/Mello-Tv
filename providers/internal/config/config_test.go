package config

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

func mockOptions() network.Options {
	return network.Options{
		Guard: network.GuardFunc(func(ctx context.Context) error {
			if ctx == nil {
				return errors.New("context cannot be nil")
			}
			return nil
		}),
	}
}

func TestNewEngine_RequiresVPNGuard(t *testing.T) {
	coord, cleanup, err := NewEngine(network.Options{}, nil)
	if err == nil {
		t.Fatal("expected error when guard is nil, got nil")
	}
	if coord != nil {
		t.Errorf("expected nil coordinator, got %v", coord)
	}
	if cleanup != nil {
		t.Error("expected nil cleanup, got non-nil func")
	}
}

func TestNewEngine_SuccessAndIdempotentCleanup(t *testing.T) {
	var closedCount atomic.Int32
	opts := mockOptions()
	opts.Cleanup = func() {
		closedCount.Add(1)
	}

	diagnosticCalls := make(chan model.ProviderStatus, 4)
	coord, cleanup, err := NewEngine(opts, func(status model.ProviderStatus) {
		select {
		case diagnosticCalls <- status:
		default:
		}
	})
	if err != nil {
		t.Fatalf("unexpected NewEngine error: %v", err)
	}
	if coord == nil {
		t.Fatal("expected non-nil coordinator")
	}
	if cleanup == nil {
		t.Fatal("expected non-nil cleanup function")
	}

	// First cleanup
	cleanup()
	if closedCount.Load() != 1 {
		t.Errorf("expected cleanup count 1, got %d", closedCount.Load())
	}

	// Second cleanup should be safe and idempotent due to sync.Once
	cleanup()
	if closedCount.Load() != 1 {
		t.Errorf("expected cleanup count to remain 1, got %d", closedCount.Load())
	}
}

func TestService_NilGuards(t *testing.T) {
	ctx := context.Background()
	var nilSvc *Service

	if err := nilSvc.Check(ctx); err == nil || err.Error() != "service is nil" {
		t.Errorf("expected 'service is nil', got %v", err)
	}
	if err := nilSvc.Ready(ctx); err == nil || err.Error() != "service is nil" {
		t.Errorf("expected 'service is nil', got %v", err)
	}
	if _, err := nilSvc.Resolve(ctx, model.Lookup{}); err == nil || err.Error() != "service is nil" {
		t.Errorf("expected 'service is nil', got %v", err)
	}
	nilSvc.Close() // Must not panic

	svc := NewService("127.0.0.1:53", nil)
	var nilCtx context.Context
	if err := svc.Check(nilCtx); err == nil || err.Error() != "context cannot be nil" {
		t.Errorf("expected 'context cannot be nil', got %v", err)
	}
	if err := svc.Ready(nilCtx); err == nil || err.Error() != "context cannot be nil" {
		t.Errorf("expected 'context cannot be nil', got %v", err)
	}
	if _, err := svc.Resolve(nilCtx, model.Lookup{}); err == nil || err.Error() != "context cannot be nil" {
		t.Errorf("expected 'context cannot be nil', got %v", err)
	}
}

func TestService_VPNUnavailable(t *testing.T) {
	svc := NewService("192.0.2.1:53", nil)
	ctx := context.Background()

	err := svc.Check(ctx)
	if err == nil || err.Error() != "VPN unavailable" {
		t.Fatalf("expected 'VPN unavailable' without tunnel, got %v", err)
	}

	if err := svc.Ready(ctx); err == nil || err.Error() != "VPN unavailable" {
		t.Errorf("expected Ready to fail with 'VPN unavailable', got %v", err)
	}

	if _, err := svc.Resolve(ctx, model.Lookup{}); err == nil || err.Error() != "VPN unavailable" {
		t.Errorf("expected Resolve to fail with 'VPN unavailable', got %v", err)
	}

	svc.Close()
}

func TestService_LifecycleWithMockOpener(t *testing.T) {
	var cleaned atomic.Bool
	svc := NewService("1.1.1.1:53", nil)
	svc.optionsOpener = func(ctx context.Context, dnsAddress string, budget *network.AcceptanceBudget) (network.Options, error) {
		opts := mockOptions()
		opts.Cleanup = func() {
			cleaned.Store(true)
		}
		return opts, nil
	}

	ctx := context.Background()

	// Initial Check performs lazy initialization
	if err := svc.Check(ctx); err != nil {
		t.Fatalf("initial Check failed: %v", err)
	}

	// Second Check hits the fast path (engine != nil)
	if err := svc.Check(ctx); err != nil {
		t.Fatalf("fast-path Check failed: %v", err)
	}

	// Ready checks both engine and budget
	if err := svc.Ready(ctx); err != nil {
		t.Fatalf("Ready failed: %v", err)
	}

	// Close shuts down the engine and calls cleanup
	svc.Close()
	if !cleaned.Load() {
		t.Error("expected service cleanup to be invoked on Close")
	}

	// Operations after Close must fail with 'service closed'
	if err := svc.Check(ctx); err == nil || err.Error() != "service closed" {
		t.Errorf("expected 'service closed' on Check, got %v", err)
	}
	if err := svc.Ready(ctx); err == nil || err.Error() != "service closed" {
		t.Errorf("expected 'service closed' on Ready, got %v", err)
	}
	if _, err := svc.Resolve(ctx, model.Lookup{}); err == nil || err.Error() != "service closed" {
		t.Errorf("expected 'service closed' on Resolve, got %v", err)
	}

	// Second Close must be safe and idempotent
	svc.Close()
}

func TestService_ConcurrentInitialization(t *testing.T) {
	svc := NewService("1.1.1.1:53", nil)
	svc.optionsOpener = func(ctx context.Context, dnsAddress string, budget *network.AcceptanceBudget) (network.Options, error) {
		time.Sleep(10 * time.Millisecond)
		return mockOptions(), nil
	}

	ctx := context.Background()
	const callers = 8
	var wg sync.WaitGroup
	wg.Add(callers)

	for i := 0; i < callers; i++ {
		go func() {
			defer wg.Done()
			_ = svc.Check(ctx)
		}()
	}
	wg.Wait()

	// After all concurrent attempts, the service must be successfully initialized
	if err := svc.Check(ctx); err != nil {
		t.Fatalf("expected initialized service after concurrent checks, got: %v", err)
	}
	svc.Close()
}

func TestService_CloseDuringInitialization(t *testing.T) {
	initStarted := make(chan struct{})
	initBlock := make(chan struct{})

	svc := NewService("1.1.1.1:53", nil)
	svc.optionsOpener = func(ctx context.Context, dnsAddress string, budget *network.AcceptanceBudget) (network.Options, error) {
		close(initStarted)
		<-initBlock
		return mockOptions(), nil
	}

	ctx := context.Background()
	var checkErr error
	var wg sync.WaitGroup
	wg.Add(1)

	go func() {
		defer wg.Done()
		checkErr = svc.Check(ctx)
	}()

	<-initStarted
	// Trigger Close while initialization is still blocked
	closeDone := make(chan struct{})
	go func() {
		svc.Close()
		close(closeDone)
	}()

	// Unblock initialization
	close(initBlock)
	wg.Wait()
	<-closeDone

	if checkErr == nil || checkErr.Error() != "service closed" {
		t.Errorf("expected Check to return 'service closed', got: %v", checkErr)
	}
}

func BenchmarkService_CheckFastPath(b *testing.B) {
	svc := NewService("1.1.1.1:53", nil)
	svc.optionsOpener = func(ctx context.Context, dnsAddress string, budget *network.AcceptanceBudget) (network.Options, error) {
		return mockOptions(), nil
	}
	ctx := context.Background()
	if err := svc.Check(ctx); err != nil {
		b.Fatalf("setup Check failed: %v", err)
	}
	defer svc.Close()

	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		if err := svc.Check(ctx); err != nil {
			b.Fatalf("Check failed: %v", err)
		}
	}
}

func BenchmarkNewEngine(b *testing.B) {
	opts := mockOptions()
	b.ResetTimer()
	b.ReportAllocs()

	for b.Loop() {
		coord, cleanup, err := NewEngine(opts, nil)
		if err != nil {
			b.Fatalf("NewEngine failed: %v", err)
		}
		cleanup()
		_ = coord
	}
}
