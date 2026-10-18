// Command factory-core starts the S1 Factory Core: durable SQLite state,
// authenticated HTTP API, event replay stream and best-effort telemetry.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/api"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "factory-core: "+err.Error())
		os.Exit(1)
	}
}

func run() error {
	addr := flag.String("addr", "127.0.0.1:8471", "listen address (loopback by default)")
	dbPath := flag.String("db", "./data/factory.db", "SQLite database path")
	webDir := flag.String("web-dir", "", "static Factory Office build to serve (empty disables)")
	flag.Parse()

	logger := telemetry.NewLogger()
	token := resolveToken(logger)

	if dir := filepath.Dir(*dbPath); dir != "." && dir != "" {
		if err := os.MkdirAll(dir, 0o750); err != nil {
			return fmt.Errorf("create data dir: %w", err)
		}
	}
	db, err := store.Open(*dbPath)
	if err != nil {
		return err
	}
	defer func() { _ = db.Close() }()

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	tracer, shutdownTracing := telemetry.InitTracing(ctx, api.Version)
	defer shutdownTracing(context.Background())

	hub := api.NewHub()
	server := api.New(db, token, logger, tracer, hub).WithWebDir(*webDir)

	boot, err := db.RecordCoreStarted(ctx, api.Version)
	if err != nil {
		return fmt.Errorf("record CoreStarted: %w", err)
	}
	hub.Publish(boot)

	httpServer := &http.Server{
		Addr:              *addr,
		Handler:           server.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
	}

	logger.Info("factory core started",
		"addr", *addr,
		"version", api.Version,
		"webDir", *webDir)

	errCh := make(chan error, 1)
	go func() {
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			errCh <- err
		}
	}()

	select {
	case <-ctx.Done():
		logger.Info("factory core stopping")
	case err := <-errCh:
		return fmt.Errorf("serve: %w", err)
	}

	shCtx, cancelShutdown := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancelShutdown()
	return httpServer.Shutdown(shCtx)
}

// resolveToken loads the API token from FACTORY_API_TOKEN. When unset, a
// random token is generated for this process and printed once to stderr so
// a Core never runs unauthenticated by accident.
func resolveToken(logger *slog.Logger) string {
	if v := os.Getenv("FACTORY_API_TOKEN"); v != "" {
		telemetry.RegisterSecret(v)
		return v
	}
	var b [32]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	generated := hex.EncodeToString(b[:])
	telemetry.RegisterSecret(generated)
	logger.Info("FACTORY_API_TOKEN unset; generated an ephemeral token for this process")
	return generated
}
