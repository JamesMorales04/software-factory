package domain

import (
	"errors"
	"time"
)

// S1 event types. Only kinds justified by the current slice exist; new
// vocabularies require a documented schema version and replay tests.
const (
	EventCoreStarted        = "CoreStarted"
	EventActionCreated      = "ActionCreated"
	EventActionTransitioned = "ActionTransitioned"
)

// EventSchemaVersion is the S1 payload contract version.
const EventSchemaVersion = "1"

// Aggregate scopes for S1 events.
const (
	AggregateCore   = "core"
	AggregateAction = "action"
)

// Event is a durable domain fact. Business state and its event are
// committed in one storage transaction; events are never sampled.
type Event struct {
	EventID       string         `json:"eventId"`
	EventType     string         `json:"eventType"`
	SchemaVersion string         `json:"schemaVersion"`
	AggregateType string         `json:"aggregateType"`
	AggregateID   string         `json:"aggregateId"`
	Sequence      int64          `json:"sequence"`
	GlobalSeq     int64          `json:"globalSeq"`
	OccurredAt    time.Time      `json:"occurredAt"`
	RecordedAt    time.Time      `json:"recordedAt"`
	CorrelationID string         `json:"correlationId"`
	CausationID   string         `json:"causationId"`
	Actor         string         `json:"actor"`
	Payload       map[string]any `json:"payload"`
}

// Validate checks an outbound event before persistence.
func (e Event) Validate() error {
	if e.EventID == "" {
		return errors.New("eventId is required")
	}
	switch e.EventType {
	case EventCoreStarted, EventActionCreated, EventActionTransitioned:
	default:
		return errors.New("unknown eventType for S1")
	}
	if e.SchemaVersion == "" {
		return errors.New("schemaVersion is required")
	}
	if e.AggregateType == "" || e.AggregateID == "" {
		return errors.New("aggregateType and aggregateId are required")
	}
	if e.Sequence < 1 {
		return errors.New("sequence must be >= 1")
	}
	if e.Payload == nil {
		return errors.New("payload must not be nil")
	}
	return nil
}
