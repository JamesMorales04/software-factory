package domain

import "testing"

func TestBuildCampaignPlanIsDeterministicAndValid(t *testing.T) {
	plan := BuildCampaignPlan(1, 1800, 2)
	if err := plan.Validate(); err != nil {
		t.Fatalf("plan invalid: %v", err)
	}
	if len(plan.Tasks) != 8 {
		t.Fatalf("task count = %d, want 8", len(plan.Tasks))
	}
	if plan.Hash() == "" || plan.Hash() != BuildCampaignPlan(1, 1800, 2).Hash() {
		t.Fatal("plan hash is not deterministic")
	}
	gates := 0
	for _, task := range plan.Tasks {
		if task.Role == RoleQualityGate {
			gates++
			if len(task.DependsOn) != 2 {
				t.Fatalf("gate dependencies = %v", task.DependsOn)
			}
		}
	}
	if gates != 2 {
		t.Fatalf("gate count = %d, want 2", gates)
	}
}

func TestCampaignPlanRejectsTampering(t *testing.T) {
	plan := BuildCampaignPlan(0, 600, 1)
	plan.Tasks[1].DependsOn = []string{"missing"}
	if err := plan.Validate(); err == nil {
		t.Fatal("expected unknown dependency rejection")
	}
	plan = BuildCampaignPlan(0, 600, 1)
	plan.Tasks[0].DependsOn = []string{plan.Tasks[3].Key}
	if err := plan.Validate(); err == nil {
		t.Fatal("expected later-cycle dependency rejection")
	}
	plan = BuildCampaignPlan(0, 600, 1)
	plan.Tasks[1].DependsOn = []string{plan.Tasks[1].Key}
	if err := plan.Validate(); err == nil {
		t.Fatal("expected self-dependency rejection")
	}
}

func TestCampaignTaskTransitionsAreClosed(t *testing.T) {
	if !TaskPending.CanTransition(TaskReady) || !TaskReady.CanTransition(TaskDispatched) {
		t.Fatal("expected normal forward transitions")
	}
	if TaskSucceeded.CanTransition(TaskReady) {
		t.Fatal("succeeded must be terminal")
	}
	if TaskFailed.CanTransition(TaskReady) != true {
		t.Fatal("failed tasks may be retried only through the bounded store path")
	}
	if TaskDispatched.CanTransition(TaskPending) {
		t.Fatal("dispatched must not return to pending")
	}
}

func TestNewCampaignParamsBounds(t *testing.T) {
	valid := NewCampaignParams{
		SpecID: "0123456789abcdef0123456789abcdef", MaxRepairCycles: 0,
		MaxWallClockSecs: 600, MaxParallel: 2,
	}
	if _, err := valid.Validate(); err != nil {
		t.Fatalf("valid params rejected: %v", err)
	}
	tooMany := valid
	tooMany.MaxRepairCycles = 4
	if _, err := tooMany.Validate(); err == nil {
		t.Fatal("expected repair bound rejection")
	}
	noScope := valid
	noScope.SpecID = "../spec"
	if _, err := noScope.Validate(); err == nil {
		t.Fatal("expected spec id rejection")
	}
}
