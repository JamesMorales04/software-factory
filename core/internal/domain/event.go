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

// S4-D Lead events carry identity and digest facts only. They intentionally
// omit request objectives, model inputs, assessment bodies, and failure text.
const (
	EventLeadRequestCreated         = "LeadRequestCreated"
	EventLeadRequestStarted         = "LeadRequestStarted"
	EventLeadRequestAutoStarted     = "LeadRequestAutoStarted"
	EventLeadRequestStartAuthorized = "LeadRequestStartAuthorized"
	EventLeadAssessmentRecorded     = "LeadAssessmentRecorded"
	EventLeadPlanFrozen             = "LeadPlanFrozen"
	EventLeadRequestBlocked         = "LeadRequestBlocked"
	EventDelegationIssued           = "DelegationIssued"
	EventDelegationRevoked          = "DelegationRevoked"
)

// S4-B campaign orchestration events. They are recorded on the campaign
// aggregate so the existing ledger and SSE replay carry native workflow
// progress without a second event store.
const (
	EventCampaignCreated                       = "CampaignCreated"
	EventCampaignStarted                       = "CampaignStarted"
	EventCampaignTaskReady                     = "CampaignTaskReady"
	EventCampaignTaskDispatched                = "CampaignTaskDispatched"
	EventCampaignTaskCompleted                 = "CampaignTaskCompleted"
	EventCampaignTaskFailed                    = "CampaignTaskFailed"
	EventCampaignTaskCancelled                 = "CampaignTaskCancelled"
	EventCampaignRepairAuthorized              = "CampaignRepairAuthorized"
	EventCampaignImplementerRecoveryAuthorized = "CampaignImplementerRecoveryAuthorized"
	EventCampaignFinished                      = "CampaignFinished"
	EventCampaignCancelled                     = "CampaignCancelled"
)

// EventSchemaVersion is the S1 payload contract version.
const EventSchemaVersion = "1"

// Aggregate scopes for S1 events, extended in S2 with run and worker.
const (
	AggregateCore        = "core"
	AggregateAction      = "action"
	AggregateRun         = "run"
	AggregateWorker      = "worker"
	AggregateSpec        = "action_spec"
	AggregateCandidate   = "candidate"
	AggregateCampaign    = "campaign"
	AggregateProject     = "project"
	AggregateLeadRequest = "lead_request"
	AggregateDelegation  = "standing_delegation"
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
		EventCampaignImplementerRecoveryAuthorized,
		EventCampaignFinished, EventCampaignCancelled,
		EventProjectRegistered, EventProjectUpdated, EventProjectStatusChanged:
	case EventLeadRequestCreated, EventLeadRequestStarted, EventLeadRequestAutoStarted,
		EventLeadRequestStartAuthorized, EventLeadAssessmentRecorded, EventLeadPlanFrozen,
		EventLeadRequestBlocked:
		if e.AggregateType != AggregateLeadRequest {
			return errors.New("lead request event must use the lead_request aggregate")
		}
		if e.Payload["requestId"] != e.AggregateID {
			return errors.New("lead request event identity must match its aggregate")
		}
		if err := validateLeadEventPayload(e.EventType, e.Payload); err != nil {
			return err
		}
	case EventDelegationIssued, EventDelegationRevoked:
		if e.AggregateType != AggregateDelegation {
			return errors.New("delegation event must use the standing_delegation aggregate")
		}
		if e.Payload["delegationId"] != e.AggregateID {
			return errors.New("delegation event identity must match its aggregate")
		}
		if err := validateLeadEventPayload(e.EventType, e.Payload); err != nil {
			return err
		}
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

func validateLeadEventPayload(eventType string, payload map[string]any) error {
	var keys []string
	switch eventType {
	case EventLeadRequestCreated:
		keys = []string{"requestId", "projectId", "requestHash", "delegationId", "projectConfigHash", "status"}
	case EventLeadRequestStarted:
		keys = []string{"requestId", "actionId", "status"}
	case EventLeadRequestAutoStarted:
		keys = []string{"requestId", "campaignId", "delegationId", "riskClass", "mode", "status"}
	case EventLeadRequestStartAuthorized:
		keys = []string{"requestId", "campaignId", "delegationId", "riskClass", "mode", "status"}
	case EventLeadAssessmentRecorded:
		keys = []string{"requestId", "assessmentId", "assessmentHash", "mechanism", "status"}
	case EventLeadPlanFrozen:
		keys = []string{"requestId", "frozenPlanId", "proposalHash", "delegationHash", "projectConfigHash", "planHash", "status"}
	case EventLeadRequestBlocked:
		keys = []string{"requestId", "status"}
	case EventDelegationIssued:
		keys = []string{"delegationId", "projectId", "contentHash", "revision", "expiresAt", "status"}
	case EventDelegationRevoked:
		keys = []string{"delegationId", "contentHash", "revokedAt", "status"}
	default:
		return errors.New("unknown Lead eventType")
	}
	if len(payload) != len(keys) {
		return errors.New("lead event payload has unexpected fields")
	}
	for _, key := range keys {
		value, ok := payload[key]
		if !ok || value == nil {
			return errors.New("lead event payload is missing a required field")
		}
		if key != "revision" {
			if _, ok := value.(string); !ok {
				return errors.New("lead event payload fields must be strings")
			}
		} else {
			validRevision := false
			switch revision := value.(type) {
			case int:
				validRevision = revision > 0
			case int64:
				validRevision = revision > 0
			case float64:
				validRevision = revision >= 1 && revision == float64(int64(revision))
			}
			if !validRevision {
				return errors.New("delegation revision must be a positive integer")
			}
		}
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
