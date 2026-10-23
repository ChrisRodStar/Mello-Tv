package network

// This acceptance-only ledger bounds admitted HTTP response allowances and
// requests in this Go process. It is separate from the Rust TCP receive ledger.
import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"syscall"
	"time"
)

type allowance struct {
	RemainingBytes    int64 `json:"remaining_bytes"`
	RemainingRequests int   `json:"remaining_requests"`
	DeadlineMS        int64 `json:"deadline_ms"`
	Stopped           bool  `json:"stopped"`
}

// AcceptanceBudget is an append-only, fsynced, exclusive-owner allowance. The
// operator creates its first JSON line once; opening never creates/resets it.
// Credits are charged in full before HTTP, never refunded on failure/restart.
type AcceptanceBudget struct {
	file      *os.File
	state     allowance // Accessed only by the command owner after Open returns.
	end       time.Time
	failed    bool
	commands  chan allowanceCommand
	stop      chan struct{}
	joined    chan struct{}
	closeOnce sync.Once
	closeErr  error
}

type allowanceCommand struct {
	bytes   int64
	debit   bool
	prepaid bool
	result  chan error
}

func OpenAcceptanceBudget(path string) (*AcceptanceBudget, error) {
	file, err := os.OpenFile(path, os.O_RDWR|os.O_APPEND|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if err != nil {
		return nil, errors.New("acceptance allowance unavailable")
	}
	reject := func() (*AcceptanceBudget, error) {
		_ = file.Close()
		return nil, errors.New("acceptance allowance rejected")
	}
	parent, parentErr := os.Stat(filepath.Dir(path))
	if parentErr != nil || !parent.IsDir() || parent.Mode().Perm() != 0700 {
		return reject()
	}
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > 1024*1024 || syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB) != nil {
		return reject()
	}
	scanner := bufio.NewScanner(file)
	var previous allowance
	lines := 0
	for scanner.Scan() {
		var current allowance
		if json.Unmarshal(scanner.Bytes(), &current) != nil || current.RemainingBytes < 0 || current.RemainingRequests < 0 || current.RemainingBytes > 1<<40 || current.RemainingRequests > 10000 {
			return reject()
		}
		if lines > 0 && (current.DeadlineMS != previous.DeadlineMS || current.RemainingBytes > previous.RemainingBytes || current.RemainingRequests > previous.RemainingRequests || previous.Stopped) {
			return reject()
		}
		previous = current
		lines++
	}
	if scanner.Err() != nil || lines == 0 || previous.Stopped || previous.RemainingBytes == 0 || previous.RemainingRequests == 0 {
		return reject()
	}
	remaining := time.Until(time.UnixMilli(previous.DeadlineMS))
	if remaining <= 0 || remaining > 24*time.Hour {
		return reject()
	}
	// Append records must start on their own line. Reject partial crash writes.
	if _, err := file.Seek(-1, io.SeekEnd); err != nil {
		return reject()
	}
	var last [1]byte
	if _, err := file.Read(last[:]); err != nil || last[0] != '\n' {
		return reject()
	}
	budget := &AcceptanceBudget{
		file:  file,
		state: previous,
		end:   time.Now().Add(remaining),

		commands: make(chan allowanceCommand),
		stop:     make(chan struct{}),
		joined:   make(chan struct{}),
	}
	go budget.run()
	return budget, nil
}

// run owns all file I/O and state; callers never hold a state mutex across disk
// operations. The unbuffered command channel has no queued work allocation.
func (b *AcceptanceBudget) run() {
	defer close(b.joined)
	defer func() {
		b.closeErr = b.file.Close()
	}()
	for {
		select {
		case <-b.stop:
			return
		case command := <-b.commands:
			err := b.check()
			if command.prepaid {
				err = b.checkWork()
			}
			if command.debit {
				if err != nil || command.bytes <= 0 || command.bytes > b.state.RemainingBytes {
					b.state.Stopped = true
					_ = b.commit()
					err = errors.New("acceptance allowance ended")
				} else {
					b.state.RemainingBytes -= command.bytes
					b.state.RemainingRequests--
					err = b.commit()
				}
			}
			command.result <- err
		}
	}
}

func (b *AcceptanceBudget) check() error {
	if b.failed || b.state.Stopped || b.state.RemainingBytes <= 0 || b.state.RemainingRequests <= 0 || time.Now().After(b.end) || time.Now().UnixMilli() >= b.state.DeadlineMS {
		return errors.New("acceptance allowance ended")
	}
	return nil
}
func (b *AcceptanceBudget) command(bytes int64, debit bool, prepaid bool) error {
	if b == nil {
		return nil
	}
	command := allowanceCommand{
		bytes:   bytes,
		debit:   debit,
		prepaid: prepaid,
		result:  make(chan error, 1),
	}
	select {
	case <-b.stop:
		return errors.New("acceptance allowance closed")
	case b.commands <- command:
	}
	select {
	case <-b.stop:
		return errors.New("acceptance allowance closed")
	case err := <-command.result:
		return err
	}
}
func (b *AcceptanceBudget) Check() error {
	return b.command(0, false, false)
}
func (b *AcceptanceBudget) Take(bytes int64) error {
	return b.command(bytes, true, false)
}

// WorkCheck protects already-reserved work without requiring unused credits.
// Deadlines, file failure and owner closure still fail closed.
func (b *AcceptanceBudget) WorkCheck() error {
	return b.command(0, false, true)
}
func (b *AcceptanceBudget) checkWork() error {
	if b.failed || time.Now().After(b.end) || time.Now().UnixMilli() >= b.state.DeadlineMS {
		return errors.New("acceptance allowance ended")
	}
	return nil
}

func (b *AcceptanceBudget) commit() error {
	if b.failed {
		return errors.New("acceptance allowance unavailable")
	}
	data, err := json.Marshal(b.state)
	if err == nil {
		data = append(data, '\n')
		var written int
		written, err = b.file.Write(data)
		if written != len(data) && err == nil {
			err = io.ErrShortWrite
		}
	}
	if err == nil {
		err = b.file.Sync()
	}
	if err != nil {
		b.failed = true
		return errors.New("acceptance allowance unavailable")
	}
	return nil
}

// Context clips every in-flight HTTP operation to the original run deadline.
func (b *AcceptanceBudget) Context(ctx context.Context) (context.Context, context.CancelFunc) {
	if b == nil {
		return context.WithCancel(ctx)
	}
	return context.WithDeadline(ctx, b.end)
}

// Close releases ownership without restoring credits. The acceptance owner must
// seal the ledger at overall completion; orderly provider restarts reuse it.
func (b *AcceptanceBudget) Close() error {
	b.closeOnce.Do(func() {
		close(b.stop)
	})
	<-b.joined
	return b.closeErr
}
