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
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/api"
	"github.com/JamesMorales04/software-factory/core/internal/lead"
	"github.com/JamesMorales04/software-factory/core/internal/orchestrator"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// Lead assessment can spend up to 30 seconds creating its session, 120 seconds
// waiting for the assessment, and 20 seconds reconciling/archiving. Leave
// additional time for Core validation and persistence before closing the HTTP
// response to the client.
const coreHTTPWriteTimeout = 300 * time.Second

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
	verifierBin := flag.String("verifier-bin", "", "independent verifier executable (default: adjacent factory-verifier or PATH)")
	reviewerBin := flag.String("reviewer-bin", "", "independent reviewer executable (default: adjacent factory-reviewer or PATH)")
	campaignInterval := flag.Duration("campaign-interval", 500*time.Millisecond, "campaign scheduler reconcile interval")
	campaignSlots := flag.Int("campaign-slots", 4, "maximum concurrent verifier/reviewer participants")
	leadPaidAuthorizationID := flag.String("lead-paid-authorization-id", "", "stable one-grant authorization identity; also required as the delegation Idempotency-Key")
	leadPaidProject := flag.String("lead-paid-project-id", "", "explicit Project approved for paid Lead assessment sessions")
	leadPaidBase := flag.String("lead-paid-base-revision", "", "exact full Git revision approved for paid Lead assessment")
	leadPaidProvider := flag.String("lead-paid-provider", "", "exact provider approved for paid Lead assessment")
	leadPaidModel := flag.String("lead-paid-model", "", "exact model approved for paid Lead assessment")
	leadPaidMode := flag.String("lead-paid-paseo-mode", "", "exact Paseo mode approved for paid Lead assessment")
	leadPaidMaxSessions := flag.Int("lead-paid-max-sessions", 0, "total Factory session ceiling approved for this Lead Project and route")
	flag.Parse()

	logger := telemetry.NewLogger()
	token := resolveToken(logger, "FACTORY_API_TOKEN")
	workerToken := resolveToken(logger, "FACTORY_WORKER_TOKEN")
	verifierToken := resolveToken(logger, "FACTORY_VERIFIER_TOKEN")
	reviewerToken := resolveToken(logger, "FACTORY_REVIEWER_TOKEN")

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
	engine := orchestrator.New(db, hub, orchestrator.Config{
		CoreURL:       "http://" + coreURL(*addr),
		VerifierToken: verifierToken, ReviewerToken: reviewerToken,
		VerifierBin: *verifierBin, ReviewerBin: *reviewerBin,
		Interval: *campaignInterval, MaxSlots: *campaignSlots, Logger: logger,
	})
	server := api.New(db, token, logger, tracer, hub).
		WithWorkerToken(workerToken).
		WithVerifierToken(verifierToken).
		WithReviewerToken(reviewerToken).
		WithWebDir(*webDir).
		WithLeadAssessor(lead.NewPaseoAssessor()).
		WithLeadPaidSessionAuthorization(api.LeadPaidSessionAuthorization{
			AuthorizationID: *leadPaidAuthorizationID,
			ProjectID:       *leadPaidProject, BaseRevision: *leadPaidBase,
			Provider: *leadPaidProvider, Model: *leadPaidModel, PaseoMode: *leadPaidMode,
			MaxSessions: *leadPaidMaxSessions,
		}).
		WithCampaignCoordinator(engine)

	boot, err := db.RecordCoreStarted(ctx, api.Version)
	if err != nil {
		return fmt.Errorf("record CoreStarted: %w", err)
	}
	hub.Publish(boot)

	// S2 crash recovery: reconcile leases orphaned by the previous
	// process before accepting Worker traffic. History survives; nothing
	// unfinished disappears silently.
	reconciled, err := db.ReconcileOnBoot(ctx, telemetry.NewCorrelationID())
	if err != nil {
		return fmt.Errorf("reconcile runs on boot: %w", err)
	}
	for _, e := range reconciled {
		hub.Publish(e)
	}
	if len(reconciled) > 0 {
		logger.Info("reconciled runs on boot", "events", len(reconciled))
	}
	interruptedLead, err := db.ReconcileLeadRequestsOnBoot(ctx)
	if err != nil {
		return fmt.Errorf("reconcile lead assessments on boot: %w", err)
	}
	if interruptedLead > 0 {
		logger.Warn("interrupted lead assessments blocked for external session reconciliation", "requests", interruptedLead)
	}

	// S2 lease sweeper: expiry and Worker liveness are Core-owned. Sweeper
	// failures are logged, never fatal: telemetry-style degradation must
	// not stop business writes.
	sweepCtx, stopSweep := context.WithCancel(context.Background())
	defer stopSweep()
	go sweepLeases(sweepCtx, db, hub, logger)
	go engine.Run(sweepCtx)

	httpServer := &http.Server{
		Addr:              *addr,
		Handler:           server.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      coreHTTPWriteTimeout,
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

// resolveToken loads a named token. When unset, a random process-local
// token is generated and registered for log redaction. Use the lifecycle
// CLI when the token must persist across Core restarts.
func resolveToken(logger *slog.Logger, env string) string {
	if v := os.Getenv(env); v != "" {
		telemetry.RegisterSecret(v)
		return v
	}
	var b [32]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	generated := hex.EncodeToString(b[:])
	telemetry.RegisterSecret(generated)
	logger.Info(env + " unset; generated an ephemeral token for this process")
	return generated
}

// coreURL turns a loopback listen address into the base URL participants
// use. A wildcard bind is mapped to loopback because the API is local-only.
func coreURL(addr string) string {
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		return addr
	}
	if host == "" || host == "0.0.0.0" || host == "::" {
		host = "127.0.0.1"
	}
	return net.JoinHostPort(host, port)
}

// sweepLeases expires timed-out leases and marks overdue Workers offline
// once per second until the Core shuts down.
func sweepLeases(ctx context.Context, db *store.DB, hub *api.Hub, logger *slog.Logger) {
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			events, err := db.ExpireLeases(context.Background())
			if err != nil {
				logger.Error("lease sweep failed", "error", err.Error())
				continue
			}
			for _, e := range events {
				hub.Publish(e)
			}
			// S3-D: fail closed only Runs whose prior external generation
			// can no longer be reconciled automatically.
			gated, err := db.ResolveBlockedPaseoRetries(context.Background())
			if err != nil {
				logger.Error("paseo retry gate failed", "error", err.Error())
				continue
			}
			for _, e := range gated {
				hub.Publish(e)
			}
		}
	}
}
