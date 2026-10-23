//
//  main.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Main CLI entrypoint for the Stream TV Catalog service. Reads environment configuration,
//  handles online database backups, binds the private HTTP listener, and manages graceful shutdown.
//

package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"

	"stv-catalog/internal/api"
	"stv-catalog/internal/db"
	"stv-catalog/internal/service"
	"stv-catalog/internal/upstream"
)

func env(key, fallback string) string {
	if val := os.Getenv(key); val != "" {
		return val
	}
	return fallback
}

func run() error {
	addr := env("CATALOG_LISTEN", "127.0.0.1:8092")
	host, _, err := net.SplitHostPort(addr)
	if err != nil {
		return errors.New("invalid CATALOG_LISTEN")
	}

	token := os.Getenv("CATALOG_PRIVATE_TOKEN")
	ip := net.ParseIP(host)
	if (ip == nil || !ip.IsLoopback()) && len(token) < 32 {
		return errors.New("non-loopback binding requires a private token of at least 32 bytes")
	}

	store, err := db.Open(env("CATALOG_DB_PATH", "data/catalog.sqlite"))
	if err != nil {
		return errors.New("cannot open catalog storage")
	}
	defer store.Close()

	for _, setting := range []struct {
		name   string
		target *int64
	}{{"CATALOG_MAX_DATA_BYTES", &store.MaxBytes}} {
		if val := os.Getenv(setting.name); val != "" {
			num, err := strconv.ParseInt(val, 10, 64)
			if err != nil || num < 1<<20 || num > 1<<30 {
				return fmt.Errorf("invalid %s", setting.name)
			}
			*setting.target = num
		}
	}

	upstreamClient := upstream.NewUpstream(os.Getenv("TMDB_READ_ACCESS_TOKEN"))
	catalogService := service.NewService(store, upstreamClient)

	listener, err := net.Listen("tcp", addr)
	if err != nil {
		return errors.New("cannot bind catalog listener")
	}

	catalogService.Start()

	server := &http.Server{
		Handler:           api.NewHandler(catalogService, token),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       60 * time.Second,
		MaxHeaderBytes:    16 << 10,
	}

	serverErrorChan := make(chan error, 1)
	go func() {
		serverErrorChan <- server.Serve(listener)
	}()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	log.Print("catalog listening; private version 1")

	var serveErr error
	select {
	case <-ctx.Done():
	case err := <-serverErrorChan:
		if !errors.Is(err, http.ErrServerClosed) {
			serveErr = err
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	workerErr := catalogService.Close(shutdownCtx)
	serverErr := server.Shutdown(shutdownCtx)
	if serverErr != nil {
		_ = server.Close()
	}

	if workerErr != nil {
		return workerErr
	}
	if serveErr != nil {
		return serveErr
	}
	return serverErr
}

func main() {
	backupPath := flag.String("backup", "", "write a new consistent SQLite backup and exit")
	flag.Parse()

	if *backupPath != "" {
		store, err := db.Open(env("CATALOG_DB_PATH", "data/catalog.sqlite"))
		if err != nil {
			log.Fatal("cannot open catalog storage")
		}
		defer store.Close()

		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()

		if err = store.Backup(ctx, *backupPath); err != nil {
			log.Fatal(err)
		}
		log.Print("catalog backup completed")
		return
	}

	if err := run(); err != nil {
		log.Print("catalog stopped: ", err)
		os.Exit(1)
	}
}
