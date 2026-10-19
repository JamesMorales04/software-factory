package store

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func openTestDB(t *testing.T) *DB {
	t.Helper()
	db, err := Open(filepath.Join(t.TempDir(), "test.db"))
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return db
}

func TestWALModeEnforced(t *testing.T) {
	db := openTestDB(t)
	mode, err := db.JournalMode()
	if err != nil {
		t.Fatalf("JournalMode: %v", err)
	}
	if mode != "wal" {
		t.Fatalf("journal_mode = %q, want wal", mode)
	}
}

func TestCreateActionPersistsActionAndEventAtomically(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	params, _ := domain.NewActionParams{Title: "First", Description: "d"}.Validate()

	action, replayed, err := db.CreateAction(ctx, params, "corr-1")
	if err != nil {
		t.Fatalf("CreateAction: %v", err)
	}
	if replayed || action.ID == "" {
		t.Fatalf("unexpected create result: replayed=%v id=%q", replayed, action.ID)
	}

	got, err := db.GetAction(action.ID)
	if err != nil {
		t.Fatalf("GetAction: %v", err)
	}
	if got.Title != "First" || got.Status != domain.StatusPending {
		t.Fatalf("round trip mismatch: %+v", got)
	}

	events, err := db.ListAggregateEvents(domain.AggregateAction, action.ID)
	if err != nil {
		t.Fatalf("ListAggregateEvents: %v", err)
	}
	if len(events) != 1 || events[0].EventType != domain.EventActionCreated {
		t.Fatalf("expected one ActionCreated, got %+v", events)
	}
	if events[0].Sequence != 1 || events[0].CorrelationID != "corr-1" {
		t.Fatalf("event linkage wrong: %+v", events[0])
	}
}

func TestCreateActionRejectsUnknownProject(t *testing.T) {
	db := openTestDB(t)
	params, _ := domain.NewActionParams{Title: "x", ProjectID: "nope"}.Validate()
	if _, _, err := db.CreateAction(context.Background(), params, ""); err == nil {
		t.Fatal("expected unknown project rejection")
	}
}

func TestIdempotentCreateReplaysWithoutNewEvent(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	params, _ := domain.NewActionParams{Title: "Once", IdempotencyKey: "key-1"}.Validate()

	first, replayed, err := db.CreateAction(ctx, params, "c1")
	if err != nil || replayed {
		t.Fatalf("first create: %v replayed=%v", err, replayed)
	}
	before, _ := db.CountEvents()
	second, replayed, err := db.CreateAction(ctx, params, "c2")
	if err != nil || !replayed {
		t.Fatalf("second create: %v replayed=%v", err, replayed)
	}
	if second.ID != first.ID {
		t.Fatalf("replay returned different action %q vs %q", second.ID, first.ID)
	}
	after, _ := db.CountEvents()
	if after != before {
		t.Fatalf("idempotent replay recorded %d new events", after-before)
	}
	if n, _ := db.CountActions(); n != 1 {
		t.Fatalf("duplicate actions persisted: %d", n)
	}
}

func TestTransitionRecordsSequencedEvent(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	params, _ := domain.NewActionParams{Title: "Flow"}.Validate()
	action, _, _ := db.CreateAction(ctx, params, "c")

	updated, event, err := db.TransitionAction(ctx, action.ID, domain.StatusInProgress, "c2")
	if err != nil {
		t.Fatalf("TransitionAction: %v", err)
	}
	if updated.Status != domain.StatusInProgress {
		t.Fatalf("status not updated: %s", updated.Status)
	}
	if event.Sequence != 2 || event.EventType != domain.EventActionTransitioned {
		t.Fatalf("bad transition event: %+v", event)
	}
	if _, _, err := db.TransitionAction(ctx, action.ID, domain.StatusPending, "c3"); err == nil {
		t.Fatal("backward transition must fail")
	}
}

func TestRestartRetainsStateAndLedger(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "restart.db")
	ctx := context.Background()

	db, err := Open(path)
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	params, _ := domain.NewActionParams{Title: "Survive"}.Validate()
	action, _, err := db.CreateAction(ctx, params, "c")
	if err != nil {
		t.Fatalf("CreateAction: %v", err)
	}
	if _, _, err := db.TransitionAction(ctx, action.ID, domain.StatusInProgress, "c"); err != nil {
		t.Fatalf("TransitionAction: %v", err)
	}
	beforeActions, _ := db.CountActions()
	beforeEvents, _ := db.CountEvents()
	_ = db.Close()

	reopened, err := Open(path)
	if err != nil {
		t.Fatalf("reopen: %v", err)
	}
	defer func() { _ = reopened.Close() }()
	if n, _ := reopened.CountActions(); n != beforeActions {
		t.Fatalf("actions lost across restart: %d vs %d", n, beforeActions)
	}
	if n, _ := reopened.CountEvents(); n != beforeEvents {
		t.Fatalf("events lost across restart: %d vs %d", n, beforeEvents)
	}
	got, err := reopened.GetAction(action.ID)
	if err != nil || got.Status != domain.StatusInProgress {
		t.Fatalf("action state wrong after restart: %+v %v", got, err)
	}
	events, err := reopened.ListAggregateEvents(domain.AggregateAction, action.ID)
	if err != nil || len(events) != 2 {
		t.Fatalf("expected 2 replayed events, got %+v %v", events, err)
	}
	if events[0].Sequence != 1 || events[1].Sequence != 2 {
		t.Fatalf("replay order broken: %+v", events)
	}
}

func TestGlobalReplayOrder(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	for _, title := range []string{"a", "b", "c"} {
		p, _ := domain.NewActionParams{Title: title}.Validate()
		if _, _, err := db.CreateAction(ctx, p, "c"); err != nil {
			t.Fatalf("CreateAction: %v", err)
		}
	}
	all, err := db.ListEvents(0, 100)
	if err != nil {
		t.Fatalf("ListEvents: %v", err)
	}
	if len(all) != 3 {
		t.Fatalf("expected 3 events, got %d", len(all))
	}
	for i := 1; i < len(all); i++ {
		if all[i].GlobalSeq <= all[i-1].GlobalSeq {
			t.Fatalf("global order violated at %d", i)
		}
	}
	page, err := db.ListEvents(all[0].GlobalSeq, 100)
	if err != nil || len(page) != 2 {
		t.Fatalf("cursor resume failed: %+v %v", page, err)
	}
}
