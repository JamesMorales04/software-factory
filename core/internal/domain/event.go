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

// S2 event types for Runs, Attempts (recorded on the run aggregate) and
// Worker observations.
const (
	EventRunQueued        = "RunQueued"
	EventRunLeased        = "RunLeased"
	EventRunStarted       = "RunStarted"
	EventRunCompleted     = "RunCompleted"
	EventRunFailed        = "RunFailed"
	EventRunExpired       = "RunExpired"
	EventRunCancelled     = "RunCancelled"
	EventWorkerRegistered = "WorkerRegistered"
	EventWorkerOffline    = "WorkerOffline"
)

// S3-A event types for the durable Paseo session binding. All are recorded
// on the run aggregate so the existing replay transport (per-aggregate and
// global) carries them with no second ledger.
const (
	EventPaseoSessionIntended  = "PaseoSessionIntended"
	EventPaseoSessionActive    = "PaseoSessionActive"
	EventPaseoSessionSucceeded = "PaseoSessionSucceeded"
	EventPaseoSessionFailed    = "PaseoSessionFailed"
	EventPaseoSessionCancelled = "PaseoSessionCancelled"
	EventPaseoSessionOrphaned  = "PaseoSessionOrphaned"
	// S3-D records the observed cleanup/reconciliation result on the same
	// run aggregate; it never rewrites the immutable orphan event.
	EventPaseoSessionRecoveryRecorded = "PaseoSessionRecoveryRecorded"
)

// S4-A event types for the frozen specification, immutable candidate,
// independent evidence and the Core Quality Gate. Candidate events are
// recorded on the candidate aggregate so replay preserves exact lineage.
const (
	EventActionSpecFrozen     = "ActionSpecFrozen"
	EventCandidateFrozen      = "CandidateFrozen"
	EventCandidateSuperseded  = "CandidateSuperseded"
	EventCandidateRejected    = "CandidateRejected"
	EventVerificationRecorded = "VerificationRecorded"
	EventReviewRecorded       = "ReviewRecorded"
	EventQualityGateDecided   = "QualityGateDecided"
)

// S4-C Project Registry events. They are recorded on the project aggregate
// so configuration revisions and status changes are replayable.
const (
	EventProjectRegistered    = "ProjectRegistered"
	EventProjectUpdated       = "ProjectUpdated"
	EventProjectStatusChanged = "ProjectStatusChanged"
)

// S4-B campaign orchestration events. They are recorded on the campaign
// aggregate so the existing ledger and SSE replay carry native workflow
// progress without a second event store.
const (
	EventCampaignCreated          = "CampaignCreated"
	EventCampaignStarted          = "CampaignStarted"
	EventCampaignTaskReady        = "CampaignTaskReady"
	EventCampaignTaskDispatched   = "CampaignTaskDispatched"
	EventCampaignTaskCompleted    = "CampaignTaskCompleted"
	EventCampaignTaskFailed       = "CampaignTaskFailed"
	EventCampaignTaskCancelled    = "CampaignTaskCancelled"
	EventCampaignRepairAuthorized = "CampaignRepairAuthorized"
	EventCampaignFinished         = "CampaignFinished"
	EventCampaignCancelled        = "CampaignCancelled"
)

// EventSchemaVersion is the S1 payload contract version.
const EventSchemaVersion = "1"

// Aggregate scopes for S1 events, extended in S2 with run and worker.
const (
	AggregateCore      = "core"
	AggregateAction    = "action"
	AggregateRun       = "run"
	AggregateWorker    = "worker"
	AggregateSpec      = "action_spec"
	AggregateCandidate = "candidate"
	AggregateCampaign  = "campaign"
	AggregateProject   = "project"
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
	case EventCoreStarted, EventActionCreated, EventActionTransitioned,
		EventRunQueued, EventRunLeased, EventRunStarted, EventRunCompleted,
		EventRunFailed, EventRunExpired, EventRunCancelled,
		EventWorkerRegistered, EventWorkerOffline,
		EventPaseoSessionIntended, EventPaseoSessionActive,
		EventPaseoSessionSucceeded, EventPaseoSessionFailed,
		EventPaseoSessionCancelled, EventPaseoSessionOrphaned,
		EventPaseoSessionRecoveryRecorded,
		EventActionSpecFrozen, EventCandidateFrozen, EventCandidateSuperseded, EventCandidateRejected,
		EventVerificationRecorded, EventReviewRecorded, EventQualityGateDecided,
		EventCampaignCreated, EventCampaignStarted, EventCampaignTaskReady,
		EventCampaignTaskDispatched, EventCampaignTaskCompleted, EventCampaignTaskFailed,
		EventCampaignTaskCancelled, EventCampaignRepairAuthorized,
		EventCampaignFinished, EventCampaignCancelled,
		EventProjectRegistered, EventProjectUpdated, EventProjectStatusChanged:
	default:
		return errors.New("unknown eventType")
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

// Projected returns a copy safe for UI/read projections. S3-B persisted a
// truncated source diff excerpt inside some result/event payloads; S3-C no
// longer collects it, and this projection stops legacy excerpts from
// crossing the read boundary. The durable ledger is never rewritten or
// deleted — only this returned copy is filtered.
func (e Event) Projected() Event {
	e.Payload = projectPayload(e.Payload)
	return e
}

func projectPayload(payload map[string]any) map[string]any {
	if payload == nil {
		return nil
	}
	out := make(map[string]any, len(payload))
	for key, value := range payload {
		if key == "diffExcerpt" {
			continue
		}
		out[key] = projectValue(value)
	}
	return out
}

func projectValue(value any) any {
	switch typed := value.(type) {
	case map[string]any:
		return projectPayload(typed)
	case []any:
		out := make([]any, len(typed))
		for i, item := range typed {
			out[i] = projectValue(item)
		}
		return out
	default:
		return value
	}
}
