package store

import (
	"context"
	"errors"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func prepareImplementer(t *testing.T, db *DB, repairs int, maxAttempts int) (domain.ActionSpec, domain.Campaign, domain.CampaignTask, domain.Run, domain.Attempt) {
	t.Helper()
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, repairs)
	var err error
	if maxAttempts > 0 {
		if _, err := db.sql.Exec(`UPDATE campaigns SET max_attempts=? WHERE id=?`, maxAttempts, campaign.ID); err != nil {
			t.Fatal(err)
		}
		campaign, err = db.GetCampaign(campaign.ID)
		if err != nil {
			t.Fatal(err)
		}
	}
	if _, _, _, err = db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, err := db.CampaignTasks(campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	var impl domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "implementer-0" {
			impl = task
		}
	}
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatal(err)
	}
	impl, err = db.getCampaignTask(campaign.ID, impl.ID)
	if err != nil {
		t.Fatal(err)
	}
	registerWorker(t, db, "local-1")
	run, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "c")
	if err != nil || !found {
		t.Fatalf("claim: %v found=%v", err, found)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	return spec, campaign, impl, run, attempt
}

func TestImplementerRecoveryPreservesFailedRunAndCreatesNewRun(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	spec, campaign, impl, run, attempt := prepareImplementer(t, db, 0, 4)
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID,
		attempt.LeaseGeneration, domain.OutcomeFailed, nil,
		"paseo-task verify: no-source-change: agent left no git diff", "c"); err != nil {
		t.Fatal(err)
	}
	used, max, err := db.CampaignAttemptsUsed(campaign.ID)
	if err != nil || used != 1 || max != 4 {
		t.Fatalf("attempts: used=%d max=%d err=%v", used, max, err)
	}
	task, events, err := db.AuthorizeImplementerRecovery(ctx, campaign.ID, impl.ID,
		impl.DispatchToken, domain.RecoverableImplementationFailure,
		"no-source-change: agent left no git diff", "c")
	if err != nil {
		t.Fatal(err)
	}
	if task.Status != domain.TaskReady || len(events) != 1 ||
		events[0].EventType != domain.EventCampaignImplementerRecoveryAuthorized {
		t.Fatalf("recovery task/events: %+v %+v", task, events)
	}
	recoveries, err := db.CampaignRecoveries(campaign.ID)
	if err != nil || len(recoveries) != 1 || recoveries[0].PreviousRunID != run.ID ||
		recoveries[0].NewRunID != "" {
		t.Fatalf("recoveries: %+v %v", recoveries, err)
	}
	oldRun, err := db.GetRun(run.ID)
	if err != nil || oldRun.Status != domain.RunFailed {
		t.Fatalf("failed run was rewritten: %+v %v", oldRun, err)
	}
	if _, newRun, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatal(err)
	} else if newRun.ID == run.ID {
		t.Fatal("recovery reused the failed Run identity")
	}
	recoveries, err = db.CampaignRecoveries(campaign.ID)
	if err != nil || recoveries[0].NewRunID == "" || recoveries[0].NewRunID == run.ID {
		t.Fatalf("recovery new run not linked: %+v %v", recoveries, err)
	}
}

func TestRecoveryRefusesStaleDispatchAndEscalationPreservesHistory(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, campaign, impl, run, attempt := prepareImplementer(t, db, 0, 0)
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID,
		attempt.LeaseGeneration, domain.OutcomeFailed, nil, "timeout", "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := db.AuthorizeImplementerRecovery(ctx, campaign.ID, impl.ID,
		"wrong-token", domain.RecoverableExecutionFailure, "timeout", "c"); !errors.Is(err, domain.ErrStaleGeneration) {
		t.Fatalf("stale dispatch token not rejected: %v", err)
	}
	finished, events, err := db.EscalateCampaign(ctx, campaign.ID, "ambiguous oracle failure", "c")
	if err != nil {
		t.Fatal(err)
	}
	if finished.Status != domain.CampaignHumanRequired || len(events) == 0 {
		t.Fatalf("escalation: %+v %+v", finished, events)
	}
	recoveries, err := db.CampaignRecoveries(campaign.ID)
	if err != nil || len(recoveries) != 0 {
		t.Fatalf("escalation created a recovery: %+v %v", recoveries, err)
	}
	oldRun, err := db.GetRun(run.ID)
	if err != nil || oldRun.Status != domain.RunFailed {
		t.Fatalf("failed run lost: %+v %v", oldRun, err)
	}
}
