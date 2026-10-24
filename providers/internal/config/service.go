package config

import (
	"context"
	"errors"
	"sync"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/search"
)

// Service lazily establishes the local guarded engine, keeping liveness usable
// while Mullvad is unavailable. A changed tunnel requires a process restart.
type Service struct {
	mu            sync.Mutex
	initializing  chan struct{}
	dnsAddress    string
	budget        *network.AcceptanceBudget
	engine        *search.Coordinator
	guard         network.Guard
	cleanup       func()
	closed        bool
	optionsOpener func(ctx context.Context, dnsAddress string, budget *network.AcceptanceBudget) (network.Options, error)
}

func NewService(dnsAddress string, budget *network.AcceptanceBudget) *Service {
	return &Service{
		dnsAddress:    dnsAddress,
		budget:        budget,
		initializing:  make(chan struct{}, 1),
		optionsOpener: network.LocalMullvadOptionsWithBudget,
	}
}

func (s *Service) Check(ctx context.Context) error {
	if s == nil {
		return errors.New("service is nil")
	}
	if ctx == nil {
		return errors.New("context cannot be nil")
	}
	s.mu.Lock()
	engine, guard, closed := s.engine, s.guard, s.closed
	s.mu.Unlock()
	if closed {
		return errors.New("service closed")
	}
	if engine != nil {
		if guard != nil {
			return guard.Check(ctx)
		}
		return nil
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
		if guard != nil {
			return guard.Check(ctx)
		}
		return nil
	}
	opener := s.optionsOpener
	if opener == nil {
		opener = network.LocalMullvadOptionsWithBudget
	}
	options, err := opener(ctx, s.dnsAddress, s.budget)
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
	if options.Guard != nil {
		return options.Guard.Check(ctx)
	}
	return nil
}

// Ready additionally checks admission; prepaid work uses Check instead.
func (s *Service) Ready(ctx context.Context) error {
	if s == nil {
		return errors.New("service is nil")
	}
	if ctx == nil {
		return errors.New("context cannot be nil")
	}
	if err := s.Check(ctx); err != nil {
		return err
	}
	if s.budget != nil {
		return s.budget.Check()
	}
	return nil
}

func (s *Service) Resolve(ctx context.Context, lookup model.Lookup) (model.Selection, error) {
	if s == nil {
		return model.Selection{}, errors.New("service is nil")
	}
	if ctx == nil {
		return model.Selection{}, errors.New("context cannot be nil")
	}
	if s.budget != nil {
		if err := s.budget.Check(); err != nil {
			return model.Selection{}, err
		}
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
	if engine == nil {
		return model.Selection{}, errors.New("engine unavailable")
	}
	return engine.Resolve(ctx, lookup)
}

func (s *Service) Close() {
	if s == nil {
		return
	}
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
