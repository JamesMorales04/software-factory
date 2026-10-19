package domain

import "testing"

func TestValidateAcceptsMinimalInput(t *testing.T) {
	p, err := NewActionParams{Title: "  Ship S1  "}.Validate()
	if err != nil {
		t.Fatalf("Validate: %v", err)
	}
	if p.Title != "Ship S1" {
		t.Fatalf("title not trimmed: %q", p.Title)
	}
	if p.ProjectID != DefaultProjectID {
		t.Fatalf("project default not applied: %q", p.ProjectID)
	}
}

func TestValidateRejectsBadInput(t *testing.T) {
	cases := []NewActionParams{
		{},
		{Title: "   "},
		{Title: "x", ProjectID: "UPPER"},
		{Title: "x", ProjectID: "has space"},
		{Title: string(make([]byte, MaxTitleLength+1))},
		{Title: "ok", Description: string(make([]byte, MaxDescriptionLength+1))},
	}
	for i, c := range cases {
		if _, err := c.Validate(); err == nil {
			t.Fatalf("case %d: expected validation error", i)
		}
	}
}

func TestTransitionMachine(t *testing.T) {
	a := Action{Status: StatusPending}
	if !a.CanTransition(StatusInProgress) || !a.CanTransition(StatusCancelled) {
		t.Fatal("pending must allow start and cancel")
	}
	if a.CanTransition(StatusCompleted) {
		t.Fatal("pending must not complete directly")
	}
	a.Status = StatusInProgress
	if !a.CanTransition(StatusCompleted) || !a.CanTransition(StatusCancelled) {
		t.Fatal("in_progress must allow complete and cancel")
	}
	a.Status = StatusCompleted
	if a.CanTransition(StatusCancelled) || a.CanTransition(StatusPending) {
		t.Fatal("completed is terminal")
	}
	if _, err := ParseStatus("bogus"); err == nil {
		t.Fatal("unknown status must be rejected")
	}
}
