// Package domain holds the S1 authoritative business rules for Projects,
// Actions and Factory Events. It depends on nothing outside the standard
// library so the rules have a single source of truth, independent of
// persistence, transport and telemetry adapters.
package domain

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// ActionStatus is the S1 Action lifecycle state.
type ActionStatus string

const (
	StatusPending    ActionStatus = "pending"
	StatusInProgress ActionStatus = "in_progress"
	StatusCompleted  ActionStatus = "completed"
	StatusCancelled  ActionStatus = "cancelled"
)

// allowedTransitions is the complete S1 state machine. Only these edges
// exist; anything else is rejected before any persistence happens.
var allowedTransitions = map[ActionStatus]map[ActionStatus]struct{}{
	StatusPending: {
		StatusInProgress: {},
		StatusCancelled:  {},
	},
	StatusInProgress: {
		StatusCompleted: {},
		StatusCancelled: {},
	},
	StatusCompleted: {},
	StatusCancelled: {},
}

// Limits are part of the S1 contract: untrusted input is bounded at the
// domain boundary, not in handlers or storage.
const (
	MaxTitleLength       = 200
	MaxDescriptionLength = 2000
	MaxProjectIDLength   = 64
)

// DefaultProjectID is the seeded project S1 operates against.
const DefaultProjectID = "default"

// Project is the minimal S1 project identity: a repository scope an Action
// belongs to. Project management beyond seed + list is deferred past S1.
type Project struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	CreatedAt time.Time `json:"createdAt"`
}

// ValidateProjectID enforces the slug shape so project references stay
// correlatable across Core, logs and UI.
func ValidateProjectID(id string) error {
	if id == "" {
		return errors.New("projectId is required")
	}
	if len(id) > MaxProjectIDLength {
		return fmt.Errorf("projectId exceeds %d characters", MaxProjectIDLength)
	}
	for _, r := range id {
		ok := r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-' || r == '_'
		if !ok {
			return errors.New("projectId must match [a-z0-9-_]")
		}
	}
	return nil
}

// Action is the S1 engineering goal record.
type Action struct {
	ID             string       `json:"id"`
	ProjectID      string       `json:"projectId"`
	Title          string       `json:"title"`
	Description    string       `json:"description"`
	Status         ActionStatus `json:"status"`
	IdempotencyKey string       `json:"idempotencyKey,omitempty"`
	CreatedAt      time.Time    `json:"createdAt"`
	UpdatedAt      time.Time    `json:"updatedAt"`
}

// NewActionParams carries validated creation input from the API boundary.
type NewActionParams struct {
	ProjectID      string
	Title          string
	Description    string
	IdempotencyKey string
}

// Validate normalizes and checks creation input. It returns a
// descriptive error; callers map it to a 400 response.
func (p NewActionParams) Validate() (NewActionParams, error) {
	p.Title = strings.TrimSpace(p.Title)
	p.Description = strings.TrimSpace(p.Description)
	p.IdempotencyKey = strings.TrimSpace(p.IdempotencyKey)
	if p.ProjectID == "" {
		p.ProjectID = DefaultProjectID
	}
	if err := ValidateProjectID(p.ProjectID); err != nil {
		return p, err
	}
	if p.Title == "" {
		return p, errors.New("title is required")
	}
	if len(p.Title) > MaxTitleLength {
		return p, fmt.Errorf("title exceeds %d characters", MaxTitleLength)
	}
	if len(p.Description) > MaxDescriptionLength {
		return p, fmt.Errorf("description exceeds %d characters", MaxDescriptionLength)
	}
	return p, nil
}

// CanTransition reports whether moving from the current status to target is
// a legal S1 edge.
func (a Action) CanTransition(to ActionStatus) bool {
	next, ok := allowedTransitions[a.Status]
	if !ok {
		return false
	}
	_, ok = next[to]
	return ok
}

// ParseStatus maps an external string to a status, rejecting unknowns.
func ParseStatus(s string) (ActionStatus, error) {
	st := ActionStatus(strings.TrimSpace(s))
	switch st {
	case StatusPending, StatusInProgress, StatusCompleted, StatusCancelled:
		return st, nil
	default:
		return "", fmt.Errorf("unknown status %q", s)
	}
}
