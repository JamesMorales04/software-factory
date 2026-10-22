package lead

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// Assessment is advisory model work. Core validates it before constructing
// any ActionSpec, plan, or Campaign. No field is an authorization decision.
type Assessment struct {
	SchemaRevision      int                     `json:"schemaRevision"`
	ProjectID           string                  `json:"projectId"`
	ActionID            string                  `json:"actionId"`
	RequestID           string                  `json:"requestId"`
	EvidenceRefs        []string                `json:"evidenceRefs"`
	Intent              string                  `json:"intent"`
	Complexity          string                  `json:"complexity"`
	EffortEstimate      EffortEstimate          `json:"effortEstimate"`
	KnowledgeGaps       []string                `json:"knowledgeGaps"`
	ChangeImpact        []string                `json:"changeImpact"`
	ValidationNeeds     []string                `json:"validationNeeds"`
	Assumptions         []string                `json:"assumptions"`
	Uncertainty         []string                `json:"uncertainty"`
	SuggestedNextAction string                  `json:"suggestedNextAction"`
	Proposal            domain.LeadPlanProposal `json:"proposal"`
}

// EffortEstimate is advisory only. Estimated ranges use coarse integer
// person-hour bounds; unknown is an explicit outcome when evidence cannot
// support a useful interval, and not_requested records that the request did
// not call for an estimate.
type EffortEstimate struct {
	Status         string   `json:"status"`
	Unit           string   `json:"unit"`
	MinPersonHours int      `json:"minPersonHours"`
	MaxPersonHours int      `json:"maxPersonHours"`
	Assumptions    []string `json:"assumptions"`
	UnknownReason  string   `json:"unknownReason"`
}

type Input struct {
	ProjectID    string
	ActionID     string
	RequestID    string
	Objective    string
	Context      map[string]string // bounded reference ID -> text
	Project      domain.Project
	Grant        domain.StandingDelegation
	BaseRevision string
}

type Observation struct {
	Mechanism   string        `json:"mechanism"`
	Provider    string        `json:"provider"`
	Model       string        `json:"model"`
	Mode        string        `json:"mode"`
	SessionID   string        `json:"sessionId,omitempty"`
	Elapsed     time.Duration `json:"elapsed"`
	InputBytes  int           `json:"inputBytes"`
	OutputBytes int           `json:"outputBytes"`
}

type Assessor interface {
	Assess(ctx context.Context, input Input) (Assessment, Observation, error)
}

func DecodeAssessment(raw []byte, input Input) (Assessment, error) {
	if err := validateAssessmentJSONShape(raw); err != nil {
		return Assessment{}, fmt.Errorf("malformed assessment: %w", err)
	}
	var a Assessment
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&a); err != nil {
		return Assessment{}, fmt.Errorf("malformed assessment: %w", err)
	}
	if err := dec.Decode(new(any)); err != io.EOF {
		return Assessment{}, errors.New("assessment contains trailing JSON")
	}
	if err := a.Validate(input); err != nil {
		return Assessment{}, err
	}
	return a, nil
}

// validateAssessmentJSONShape enforces JSON Schema's required-property,
// non-null, and unique-object-key behavior before encoding/json applies its
// more permissive struct decoding rules.
func validateAssessmentJSONShape(raw []byte) error {
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := consumeUniqueJSONValue(dec); err != nil {
		return err
	}
	if _, err := dec.Token(); err != io.EOF {
		if err == nil {
			return errors.New("assessment contains trailing JSON")
		}
		return err
	}

	var root map[string]json.RawMessage
	if err := json.Unmarshal(raw, &root); err != nil {
		return err
	}
	if err := requireJSONKeys(root,
		"schemaRevision", "projectId", "actionId", "requestId", "evidenceRefs", "intent", "complexity",
		"effortEstimate", "knowledgeGaps", "changeImpact", "validationNeeds", "assumptions", "uncertainty", "suggestedNextAction", "proposal",
	); err != nil {
		return err
	}
	var effort map[string]json.RawMessage
	if err := json.Unmarshal(root["effortEstimate"], &effort); err != nil || effort == nil {
		return errors.New("assessment effortEstimate must be an object")
	}
	if err := requireJSONKeys(effort, "status", "unit", "minPersonHours", "maxPersonHours", "assumptions", "unknownReason"); err != nil {
		return err
	}
	var proposal map[string]json.RawMessage
	if err := json.Unmarshal(root["proposal"], &proposal); err != nil || proposal == nil {
		return errors.New("assessment proposal must be an object")
	}
	if err := requireJSONKeys(proposal,
		"projectId", "actionId", "baseRevision", "projectConfigHash", "objective", "class", "assumptions", "risks",
		"acceptanceCriteria", "negativeCases", "qualityProfile", "riskClass", "budget", "provider", "model", "paseoMode",
		"maxRepairCycles", "maxWallClockSecs", "maxParallel", "maxSessions", "workUnits",
	); err != nil {
		return err
	}
	var budget map[string]json.RawMessage
	if err := json.Unmarshal(proposal["budget"], &budget); err != nil || budget == nil {
		return errors.New("assessment proposal budget must be an object")
	}
	if err := requireJSONKeys(budget, "maxAttempts", "timeoutSecs"); err != nil {
		return err
	}
	if err := validateRequiredObjectArray(proposal["acceptanceCriteria"], "id", "kind", "description", "command", "expectExit", "timeoutSecs"); err != nil {
		return fmt.Errorf("assessment acceptance criteria: %w", err)
	}
	if err := validateRequiredObjectArray(proposal["negativeCases"], "id", "kind", "description", "command", "expectExit", "timeoutSecs"); err != nil {
		return fmt.Errorf("assessment negative cases: %w", err)
	}
	if err := validateRequiredObjectArray(proposal["workUnits"], "id", "kind", "dependsOn", "allowedPaths", "capabilities"); err != nil {
		return fmt.Errorf("assessment work units: %w", err)
	}
	return nil
}

func validateRequiredObjectArray(raw json.RawMessage, required ...string) error {
	var objects []map[string]json.RawMessage
	if err := json.Unmarshal(raw, &objects); err != nil || objects == nil {
		return errors.New("must be an array of objects")
	}
	for i, object := range objects {
		if object == nil {
			return fmt.Errorf("item %d must be an object", i)
		}
		if err := requireJSONKeys(object, required...); err != nil {
			return fmt.Errorf("item %d: %w", i, err)
		}
	}
	return nil
}

func requireJSONKeys(object map[string]json.RawMessage, required ...string) error {
	for _, key := range required {
		value, ok := object[key]
		if !ok {
			return fmt.Errorf("assessment is missing required property %q", key)
		}
		if strings.TrimSpace(string(value)) == "null" {
			return fmt.Errorf("assessment property %q cannot be null", key)
		}
	}
	return nil
}

func consumeUniqueJSONValue(dec *json.Decoder) error {
	token, err := dec.Token()
	if err != nil {
		return err
	}
	delim, ok := token.(json.Delim)
	if !ok {
		if token == nil {
			return errors.New("assessment values cannot be null")
		}
		return nil
	}
	switch delim {
	case '{':
		seen := map[string]struct{}{}
		for dec.More() {
			keyToken, err := dec.Token()
			if err != nil {
				return err
			}
			key, ok := keyToken.(string)
			if !ok {
				return errors.New("assessment object key is not a string")
			}
			if _, exists := seen[key]; exists {
				return fmt.Errorf("assessment contains duplicate property %q", key)
			}
			seen[key] = struct{}{}
			if err := consumeUniqueJSONValue(dec); err != nil {
				return err
			}
		}
		_, err = dec.Token()
		return err
	case '[':
		for dec.More() {
			if err := consumeUniqueJSONValue(dec); err != nil {
				return err
			}
		}
		_, err = dec.Token()
		return err
	default:
		return errors.New("assessment contains an invalid JSON delimiter")
	}
}

func (a Assessment) Validate(input Input) error {
	if a.SchemaRevision != 1 || a.ProjectID != input.ProjectID || a.ActionID != input.ActionID || a.RequestID != input.RequestID {
		return errors.New("assessment revision or planning identity mismatch")
	}
	if len(a.EvidenceRefs) == 0 || len(a.EvidenceRefs) > 32 {
		return errors.New("assessment requires bounded evidence references")
	}
	seen := map[string]bool{}
	for _, ref := range a.EvidenceRefs {
		if _, ok := input.Context[ref]; !ok || seen[ref] {
			return errors.New("assessment cites missing or duplicate evidence")
		}
		seen[ref] = true
	}
	switch a.Intent {
	case "change", "ambiguous", "information", "audit":
	default:
		return errors.New("assessment intent is unsupported")
	}
	switch a.Complexity {
	case "small", "medium", "large", "unknown":
	default:
		return errors.New("assessment complexity is unsupported")
	}
	for _, notes := range [][]string{a.KnowledgeGaps, a.ChangeImpact, a.ValidationNeeds, a.Assumptions, a.Uncertainty} {
		if len(notes) > 16 {
			return errors.New("assessment notes exceed bound")
		}
		for _, note := range notes {
			if strings.TrimSpace(note) == "" || len(note) > 500 {
				return errors.New("assessment note is empty or unbounded")
			}
		}
	}
	if strings.TrimSpace(a.SuggestedNextAction) == "" || len(a.SuggestedNextAction) > 500 {
		return errors.New("assessment next action is empty or unbounded")
	}
	if err := a.EffortEstimate.Validate(); err != nil {
		return err
	}
	requiresEffortEstimate := a.Intent == "change" || requestsEffortEstimate(input.Objective)
	if requiresEffortEstimate && a.EffortEstimate.Status == "not_requested" {
		return errors.New("assessment omitted a requested effort estimate")
	}
	if !requiresEffortEstimate && a.EffortEstimate.Status != "not_requested" {
		return errors.New("assessment supplied an unrequested effort estimate")
	}
	if a.Intent == "change" {
		if a.Proposal.ProjectID != input.ProjectID || a.Proposal.ActionID != input.ActionID ||
			a.Proposal.ProjectConfigHash != input.Project.ConfigHash || a.Proposal.BaseRevision != input.BaseRevision {
			return errors.New("assessment proposal binding mismatch")
		}
	} else if len(a.Proposal.WorkUnits) != 0 {
		return errors.New("non-change intent cannot propose executable work")
	}
	return nil
}

func requestsEffortEstimate(objective string) bool {
	objective = strings.ToLower(objective)
	return strings.Contains(objective, "estimate") || strings.Contains(objective, "how long") ||
		(strings.Contains(objective, "effort") && strings.Contains(objective, "range"))
}

func (e EffortEstimate) Validate() error {
	if len(e.Assumptions) > 16 {
		return errors.New("assessment effort assumptions exceed bound")
	}
	for _, assumption := range e.Assumptions {
		if strings.TrimSpace(assumption) == "" || len(assumption) > 500 {
			return errors.New("assessment effort assumption is empty or unbounded")
		}
	}
	switch e.Status {
	case "estimated":
		if e.Unit != "person-hours" || e.MinPersonHours < 1 || e.MaxPersonHours <= e.MinPersonHours || e.MaxPersonHours > 10000 || len(e.Assumptions) == 0 || strings.TrimSpace(e.UnknownReason) != "" {
			return errors.New("assessment effort interval is invalid or lacks assumptions")
		}
	case "unknown":
		if e.Unit != "" || e.MinPersonHours != 0 || e.MaxPersonHours != 0 || strings.TrimSpace(e.UnknownReason) == "" || len(e.UnknownReason) > 500 {
			return errors.New("assessment unknown effort must include a bounded reason and no interval")
		}
	case "not_requested":
		if e.Unit != "" || e.MinPersonHours != 0 || e.MaxPersonHours != 0 || len(e.Assumptions) != 0 || strings.TrimSpace(e.UnknownReason) != "" {
			return errors.New("unrequested effort estimate must be empty")
		}
	default:
		return errors.New("assessment effort status is unsupported")
	}
	return nil
}
