package config

import (
	"context"
	"errors"
	"sync"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/search"
)

// Service lazily establishes the local guarded engine, keeping liveness usable
// while Mullvad is unavailable. A changed tunnel requires a process restart.
type Service struct {
	mu           sync.Mutex
	initializing chan struct{}
	dnsAddress   string
	budget       *network.AcceptanceBudget
	engine       *search.Coordinator
	guard        network.Guard
	cleanup      func()
	closed       bool
}

func NewService(dnsAddress string, budget *network.AcceptanceBudget) *Service {
	return &Service{
		dnsAddress:   dnsAddress,
		budget:       budget,
		initializing: make(chan struct{}, 1),
	}
}

func (s *Service) Check(ctx context.Context) error {
	s.mu.Lock()
	engine, guard, closed := s.engine, s.guard, s.closed
	s.mu.Unlock()
	if closed {
		return errors.New("service closed")
	}
	if engine != nil {
		return guard.Check(ctx)
	}
	select {
	case s.initializing <- struct{}{}:
		defer func() {
			<-s.initializing
		}()
	default:
		return errors.New("guard initialization in progress")
	}
	// Recheck after acquiring initialization ownership. No I/O holds the mutex.
	s.mu.Lock()
	engine, guard, closed = s.engine, s.guard, s.closed
	s.mu.Unlock()
	if closed {
		return errors.New("service closed")
	}
	if engine != nil {
		return guard.Check(ctx)
	}
	options, err := network.LocalMullvadOptionsWithBudget(ctx, s.dnsAddress, s.budget)
	if err != nil {
		return errors.New("VPN unavailable")
	}
	engine, cleanup, err := NewEngine(options, nil)
	if err != nil {
		if options.Cleanup != nil {
			options.Cleanup()
		}
		return errors.New("engine unavailable")
	}
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		cleanup()
		return errors.New("service closed")
	}
	s.engine, s.cleanup, s.guard = engine, cleanup, options.Guard
	s.mu.Unlock()
	return options.Guard.Check(ctx)
}

// Ready additionally checks admission; prepaid work uses Check instead.
func (s *Service) Ready(ctx context.Context) error {
	if err := s.Check(ctx); err != nil {
		return err
	}
	return s.budget.Check()
}

func (s *Service) Resolve(ctx context.Context, lookup model.Lookup) (model.Selection, error) {
	if err := s.budget.Check(); err != nil {
		return model.Selection{}, err
	}
	if err := s.Check(ctx); err != nil {
		return model.Selection{}, err
	}
	s.mu.Lock()
	engine, closed := s.engine, s.closed
	s.mu.Unlock()
	if closed {
		return model.Selection{}, errors.New("service closed")
	}
	return engine.Resolve(ctx, lookup)
}

func (s *Service) Close() {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return
	}
	s.closed = true
	cleanup := s.cleanup
	s.mu.Unlock()
	// Join any lazy initialization before returning to the process owner.
	s.initializing <- struct{}{}
	<-s.initializing
	if cleanup != nil {
		cleanup()
	}
}
