// s4d-semantic-collect runs exactly one real assessment session per frozen
// CAP-014 fixture and records raw output/session evidence. It is fail-closed
// and requires an explicit campaign-scoped authorization environment.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/lead"
)

const (
	maxSessions   = 11
	routeProvider = "opencode"
	routeModel    = "opencode-go/deepseek-v4.1-flash"
	routeMode     = "build"
)

type manifestEntry struct {
	FixtureID   string `json:"fixtureId"`
	Status      string `json:"status"`
	SessionID   string `json:"sessionId,omitempty"`
	ElapsedMS   int64  `json:"elapsedMs,omitempty"`
	InputBytes  int    `json:"inputBytes,omitempty"`
	OutputBytes int    `json:"outputBytes,omitempty"`
	Error       string `json:"error,omitempty"`
	StopExit    int    `json:"stopExit,omitempty"`
	ArchiveExit int    `json:"archiveExit,omitempty"`
}

func idFor(value string) string {
	sum := sha256.Sum256([]byte(value))
	return hex.EncodeToString(sum[:16])
}

func requireAuthorization() (string, error) {
	if os.Getenv("FACTORY_S4D_SEMANTIC_LIVE") != "1" {
		return "", fmt.Errorf("FACTORY_S4D_SEMANTIC_LIVE=1 is required")
	}
	authorizationID := strings.TrimSpace(os.Getenv("FACTORY_S4D_SEMANTIC_AUTHORIZATION_ID"))
	if authorizationID == "" {
		return "", fmt.Errorf("FACTORY_S4D_SEMANTIC_AUTHORIZATION_ID is required")
	}
	if budget := strings.TrimSpace(os.Getenv("FACTORY_LIVE_AUTH_SESSION_BUDGET")); budget != "11" {
		return "", fmt.Errorf("FACTORY_LIVE_AUTH_SESSION_BUDGET must be 11")
	}
	if route := os.Getenv("FACTORY_S4D_SEMANTIC_ROUTE"); route != routeProvider+"/"+routeModel+"/"+routeMode {
		return "", fmt.Errorf("authorized route must be exactly %s/%s/%s", routeProvider, routeModel, routeMode)
	}
	return authorizationID, nil
}

func main() {
	corpusPath := flag.String("corpus", "core/internal/lead/testdata/s4d-evaluation-corpus.json", "frozen corpus")
	outDir := flag.String("out", "", "evidence output directory")
	flag.Parse()
	if strings.TrimSpace(*outDir) == "" {
		fmt.Fprintln(os.Stderr, "-out is required")
		os.Exit(2)
	}
	authorizationID, err := requireAuthorization()
	if err != nil {
		fmt.Fprintln(os.Stderr, "authorization:", err)
		os.Exit(2)
	}
	raw, err := os.ReadFile(*corpusPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "corpus:", err)
		os.Exit(2)
	}
	var corpus lead.EvaluationCorpus
	if err := json.Unmarshal(raw, &corpus); err != nil {
		fmt.Fprintln(os.Stderr, "corpus:", err)
		os.Exit(2)
	}
	if len(corpus.Fixtures) != maxSessions {
		fmt.Fprintln(os.Stderr, "corpus must contain exactly 11 fixtures")
		os.Exit(2)
	}
	if err := os.MkdirAll(*outDir, 0o700); err != nil {
		fmt.Fprintln(os.Stderr, "output:", err)
		os.Exit(2)
	}
	digest := sha256.Sum256(raw)
	manifest := map[string]any{
		"corpus": corpus.ID, "corpusSha256": hex.EncodeToString(digest[:]),
		"authorizationId": authorizationID,
		"candidateSha":    strings.TrimSpace(os.Getenv("FACTORY_LIVE_AUTH_FACTORY_SHA")),
		"route":           routeProvider + "/" + routeModel + "/" + routeMode,
		"maxSessions":     maxSessions, "sessionsUsed": 0, "entries": []manifestEntry{},
	}
	entries := make([]manifestEntry, 0, maxSessions)
	used := 0
	for _, fixture := range corpus.Fixtures {
		if used >= maxSessions {
			break
		}
		used++
		requestID := idFor("request:" + fixture.ID)
		input := lead.Input{
			ProjectID: "factory-fixture-s4d-semantic", ActionID: idFor("action:" + fixture.ID),
			RequestID: requestID, Objective: fixture.Request, Context: fixture.Evidence,
			Project: domain.Project{
				ID: "factory-fixture-s4d-semantic", ConfigHash: idFor("config") + idFor("config2"),
				WorkspaceScope: ".", QualityProfile: domain.ProfileNodeBasic,
				Capabilities: []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
			},
			Grant: domain.StandingDelegation{
				ID: idFor("delegation"), ProjectID: "factory-fixture-s4d-semantic",
				ObjectiveClasses: []domain.LeadObjectiveClass{domain.LeadFeature, domain.LeadMaintenance, domain.LeadRefactor},
				AllowedPaths:     []string{"."},
				Capabilities:     []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
				RiskClasses:      []domain.RiskClass{domain.RiskR0},
				Provider:         routeProvider, Model: routeModel, PaseoMode: routeMode,
				MaxAttempts: 1, MaxTimeoutSecs: 120, MaxRepairCycles: 0, MaxWallClockSecs: 300,
				MaxParallel: 1, MaxSessions: 1, MaxRequests: 1, MaxCumulativeSessions: 1,
				MaxCumulativeAttempts: 1, ExpiresAt: time.Now().UTC().Add(time.Hour),
			},
			BaseRevision: strings.Repeat("a", 40),
		}
		started := time.Now()
		assessment, observation, assessErr := lead.NewPaseoAssessor().Assess(context.Background(), input)
		entry := manifestEntry{FixtureID: fixture.ID, ElapsedMS: time.Since(started).Milliseconds()}
		if assessErr != nil {
			entry.Status = "failed"
			entry.Error = assessErr.Error()
		} else {
			entry.Status = "captured"
			entry.SessionID = observation.SessionID
			entry.ElapsedMS = observation.Elapsed.Milliseconds()
			entry.InputBytes, entry.OutputBytes = observation.InputBytes, observation.OutputBytes
			body, _ := json.MarshalIndent(assessment, "", "  ")
			_ = os.WriteFile(filepath.Join(*outDir, fixture.ID+".assessment.json"), append(body, '\n'), 0o600)
		}
		observed, _ := json.MarshalIndent(observation, "", "  ")
		_ = os.WriteFile(filepath.Join(*outDir, fixture.ID+".observation.json"), append(observed, '\n'), 0o600)
		if entry.SessionID != "" {
			stop := exec.Command("paseo", "stop", entry.SessionID)
			archive := exec.Command("paseo", "archive", entry.SessionID)
			if err := stop.Run(); err == nil {
				entry.StopExit = 0
			} else {
				entry.StopExit = 1
			}
			if err := archive.Run(); err == nil {
				entry.ArchiveExit = 0
			} else {
				entry.ArchiveExit = 1
			}
		}
		entries = append(entries, entry)
		manifest["sessionsUsed"] = used
		manifest["entries"] = entries
		encoded, _ := json.MarshalIndent(manifest, "", "  ")
		_ = os.WriteFile(filepath.Join(*outDir, "manifest.json"), append(encoded, '\n'), 0o600)
		fmt.Fprintf(os.Stderr, "[%d/%d] %s %s\n", used, maxSessions, fixture.ID, entry.Status)
		if assessErr != nil && strings.Contains(strings.ToLower(assessErr.Error()), "session") &&
			strings.Contains(strings.ToLower(assessErr.Error()), "discovery") {
			fmt.Fprintln(os.Stderr, "stopping on unresolved external session state")
			os.Exit(2)
		}
	}
	if used != maxSessions {
		fmt.Fprintln(os.Stderr, "incomplete session budget usage")
		os.Exit(2)
	}
	failed := 0
	for _, entry := range entries {
		if entry.Status != "captured" {
			failed++
		}
	}
	if failed > 0 {
		os.Exit(1)
	}
}
