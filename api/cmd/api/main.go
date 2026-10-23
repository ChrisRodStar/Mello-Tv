package main

import (
	"context"
	"family-stream-api/internal/api"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

func run() error {
	if len(os.Args) != 2 {
		return fmt.Errorf("usage: api /private/config.json")
	}
	c, e := api.LoadConfig(os.Args[1])
	if e != nil {
		return fmt.Errorf("configuration rejected")
	}
	s, e := api.New(c)
	if e != nil {
		return e
	}
	defer s.Close()
	listener, e := net.Listen("tcp", c.Listen)
	if e != nil {
		return fmt.Errorf("listen unavailable")
	}
	defer listener.Close()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	s.Start()
	server := &http.Server{ErrorLog: log.New(io.Discard, "", 0), Handler: s.Handler(), ReadHeaderTimeout: 3 * time.Second, ReadTimeout: 5 * time.Second, WriteTimeout: 45 * time.Second, IdleTimeout: 30 * time.Second, MaxHeaderBytes: 8192}
	defer server.Close()
	done := make(chan error, 1)
	go func() { done <- server.Serve(api.LimitConnections(listener, 64)) }()
	select {
	case e = <-done:
		if e != http.ErrServerClosed {
			return fmt.Errorf("HTTP server stopped")
		}
	case <-ctx.Done():
	}
	s.StopAdmission()
	shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if e = server.Shutdown(shutdown); e != nil {
		_ = server.Close()
	}
	return nil
}
func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, "api:", e)
		os.Exit(1)
	}
}
