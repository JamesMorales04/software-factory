package api

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"os/exec"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/lead"
	projectreg "github.com/JamesMorales04/software-factory/core/internal/project"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

func leadID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

type delegationRequest struct {
	ProjectID        string                      `json:"projectId"`
	Revision         int                         `json:"revision"`
	ObjectiveClasses []domain.LeadObjectiveClass `json:"objectiveClasses"`
	AllowedPaths     []string                    `json:"allowedPaths"`
	Capabilities     []domain.Capability         `json:"capabilities"`
	RiskClasses      []domain.RiskClass          `json:"riskClasses"`
	Provider         string                      `json:"provider"`
	Model            string                      `json:"model"`
	PaseoMode        string                      `json:"paseoMode"`
	MaxAttempts      int                         `json:"maxAttempts"`
	MaxTimeoutSecs   int                         `json:"maxTimeoutSecs"`
	MaxRepairCycles  int                         `json:"maxRepairCycles"`
	MaxWallClockSecs int                         `json:"maxWallClockSecs"`
	MaxParallel      int                         `json:"maxParallel"`
	MaxSessions      int                         `json:"maxSessions"`
	ExpiresAt        time.Time                   `json:"expiresAt"`
}

func (s *Server) handleCreateDelegation(w http.ResponseWriter, r *http.Request) {
	var req delegationRequest
	if !decodeBody(w, r, &req) {
		return
	}
	key := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
	if key == "" || key != s.leadPaidSessionAuthorization.AuthorizationID {
		writeError(w, http.StatusForbidden, "unauthorized", "delegation idempotency key must match the configured paid-session authorization identity")
		return
	}
	grant := domain.StandingDelegation{
		ID: leadID(), PrincipalID: store.ActorLocalOperator, AuditID: leadID(),
		ProjectID: req.ProjectID, Revision: req.Revision, ObjectiveClasses: req.ObjectiveClasses,
		AllowedPaths: req.AllowedPaths, Capabilities: req.Capabilities, RiskClasses: req.RiskClasses,
		Provider: req.Provider, Model: req.Model, PaseoMode: req.PaseoMode,
		MaxAttempts: req.MaxAttempts, MaxTimeoutSecs: req.MaxTimeoutSecs,
		MaxRepairCycles: req.MaxRepairCycles, MaxWallClockSecs: req.MaxWallClockSecs,
		MaxParallel: req.MaxParallel, MaxSessions: req.MaxSessions, ExpiresAt: req.ExpiresAt,
	}
	project, err := s.db.GetProject(grant.ProjectID)
	if err != nil {
		mapDomainError(w, err, s.log, "delegation project lookup")
		return
	}
	inspected, err := projectreg.Inspect(r.Context(), project.RepositoryPath, project.BaseRefs)
	if err != nil || inspected.Identity != project.RepositoryIdentity {
		writeError(w, http.StatusForbidden, "unauthorized", "Project repository binding is unavailable for paid-session authorization")
		return
	}
	base := inspected.BaseTips[project.BaseRefs[0]]
	if !s.leadPaidSessionAuthorization.allows(project.ID, base, grant) {
		writeError(w, http.StatusForbidden, "unauthorized", "delegation exceeds the configured paid-session project, base, route, or session ceiling")
		return
	}
	stored, replayed, err := s.db.CreateStandingDelegation(r.Context(), grant, key)
	if err != nil {
		mapDomainError(w, err, s.log, "create delegation")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
	} else {
		s.publishLeadEvent(r.Context(), domain.AggregateDelegation, stored.ID, domain.EventDelegationIssued)
	}
	status := http.StatusCreated
	if replayed {
		status = http.StatusOK
	}
	writeJSON(w, status, map[string]any{"delegation": stored})
}

func (s *Server) handleGetDelegation(w http.ResponseWriter, r *http.Request) {
	grant, err := s.db.GetStandingDelegation(r.Context(), r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get delegation")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"delegation": grant})
}

func (s *Server) handleRevokeDelegation(w http.ResponseWriter, r *http.Request) {
	var body struct{}
	if !decodeBody(w, r, &body) {
		return
	}
	grant, changed, err := s.db.RevokeStandingDelegation(r.Context(), r.PathValue("id"), time.Now().UTC())
	if err != nil {
		mapDomainError(w, err, s.log, "revoke delegation")
		return
	}
	if !changed {
		w.Header().Set("Idempotent-Replayed", "true")
	}
	if changed && s.campaigns != nil {
		s.campaigns.Wake()
	}
	if changed {
		s.publishLeadEvent(r.Context(), domain.AggregateDelegation, grant.ID, domain.EventDelegationRevoked)
	}
	writeJSON(w, http.StatusOK, map[string]any{"delegation": grant})
}

type createLeadRequest struct {
	ProjectID    string `json:"projectId"`
	Objective    string `json:"objective"`
	DelegationID string `json:"delegationId"`
}

func (s *Server) handleCreateLeadRequest(w http.ResponseWriter, r *http.Request) {
	var req createLeadRequest
	if !decodeBody(w, r, &req) {
		return
	}
	key := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
	if key == "" {
		writeError(w, http.StatusBadRequest, "invalid", "Idempotency-Key is required for lead intake")
		return
	}
	project, err := s.db.GetProject(req.ProjectID)
	if err != nil {
		mapDomainError(w, err, s.log, "lead project lookup")
		return
	}
	grant, err := s.db.GetStandingDelegation(r.Context(), req.DelegationID)
	if err != nil || grant.ProjectID != project.ID || grant.Validate(time.Now().UTC()) != nil {
		writeError(w, http.StatusForbidden, "unauthorized", "delegation is unavailable, inactive, or outside this Project")
		return
	}
	encoded, _ := json.Marshal(req)
	request, replayed, err := s.db.CreateLeadRequest(r.Context(), store.NewLeadRequest{
		ProjectID: req.ProjectID, Objective: req.Objective, RequestJSON: string(encoded),
		IdempotencyKey: key, DelegationID: req.DelegationID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil {
		mapDomainError(w, err, s.log, "lead intake")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
	} else {
		s.publishLeadEvent(r.Context(), domain.AggregateLeadRequest, request.ID, domain.EventLeadRequestCreated)
	}
	if request.Status == "submitted" {
		s.processLeadRequest(r.Context(), request)
	}
	s.writeLeadRequest(w, r, request.ID, replayed)
}

func (s *Server) processLeadRequest(ctx context.Context, request store.LeadRequest) {
	block := func(stage, reason string) {
		_, changed, err := s.db.BlockLeadRequest(context.Background(), request.ID, stage, reason)
		if err == nil && changed {
			s.publishLeadEvent(context.Background(), domain.AggregateLeadRequest, request.ID, domain.EventLeadRequestBlocked)
		}
	}
	objective := strings.TrimSpace(request.Objective)
	title := objective
	if len(title) > domain.MaxTitleLength {
		title = title[:domain.MaxTitleLength]
	}
	action, _, err := s.db.CreateLeadAction(ctx, request.ID, domain.NewActionParams{
		ProjectID: request.ProjectID, Title: title, Description: objective,
		IdempotencyKey: "lead-action-" + request.ID,
	}, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		block("submitted", "Action creation failed")
		return
	}
	_, ok, err := s.db.ClaimLeadRequest(ctx, request.ID)
	if err != nil || !ok {
		return
	}
	s.publishLeadEvent(ctx, domain.AggregateLeadRequest, request.ID, domain.EventLeadRequestStarted)
	if s.leadAssessor == nil {
		block("assessing", "No model assessor is configured")
		return
	}
	project, err := s.db.GetProject(request.ProjectID)
	if err != nil {
		block("assessing", "Project is unavailable")
		return
	}
	if !project.Active() || project.ConfigHash != request.ProjectConfigHash {
		block("assessing", "Project configuration changed after Lead reservation")
		return
	}
	grant, err := s.db.GetStandingDelegation(ctx, request.DelegationID)
	if err != nil || grant.ProjectID != project.ID || grant.Validate(time.Now().UTC()) != nil {
		block("assessing", "Delegation is unavailable or invalid")
		return
	}
	inspected, err := projectreg.Inspect(ctx, project.RepositoryPath, project.BaseRefs)
	if err != nil || inspected.Identity != project.RepositoryIdentity {
		block("assessing", "Repository identity changed")
		return
	}
	base := inspected.BaseTips[project.BaseRefs[0]]
	if !s.leadPaidSessionAuthorization.allows(project.ID, base, grant) {
		block("assessing", "No matching bounded authorization for a paid Lead assessment session")
		return
	}
	contextRefs, err := leadContext(ctx, project.RepositoryPath, base)
	if err != nil {
		block("assessing", "Repository context is unavailable")
		return
	}
	// Re-read the binding immediately before the external session. A project
	// update or delegation revocation after request reservation must not turn
	// a stale request into a paid assessment.
	currentProject, err := s.db.GetProject(request.ProjectID)
	if err != nil || !currentProject.Active() || currentProject.ConfigHash != request.ProjectConfigHash ||
		currentProject.ConfigHash != project.ConfigHash {
		block("assessing", "Project configuration changed before Lead assessment")
		return
	}
	currentGrant, err := s.db.GetStandingDelegation(ctx, request.DelegationID)
	if err != nil || currentGrant.ProjectID != currentProject.ID || currentGrant.ContentHash != grant.ContentHash ||
		currentGrant.Validate(time.Now().UTC()) != nil || !s.leadPaidSessionAuthorization.allows(currentProject.ID, base, currentGrant) {
		block("assessing", "Delegation or paid-session authorization changed before Lead assessment")
		return
	}
	input := lead.Input{
		ProjectID: project.ID, ActionID: action.ID, RequestID: request.ID,
		Objective: objective, Context: contextRefs, Project: currentProject, Grant: currentGrant, BaseRevision: base,
	}
	assessment, observation, err := s.leadAssessor.Assess(ctx, input)
	if err != nil {
		block("assessing", boundedAssessmentFailure(err))
		return
	}
	if err := assessment.Validate(input); err != nil {
		block("assessing", "Model assessment rejected: "+boundedReason(err))
		return
	}
	assessmentJSON, _ := json.Marshal(assessment)
	routeJSON, _ := json.Marshal(observation)
	record, assessmentReplayed, err := s.db.CreateLeadAssessment(ctx, request.ID, "MODEL", string(assessmentJSON), string(routeJSON), "lead-assess-"+request.ID)
	if err != nil {
		block("assessing", "Assessment could not be recorded")
		return
	}
	if !assessmentReplayed {
		s.publishLeadEvent(ctx, domain.AggregateLeadRequest, request.ID, domain.EventLeadAssessmentRecorded)
	}
	if assessment.Intent != "change" {
		block("assessing", "Objective needs owner clarification or is not executable")
		return
	}
	if err := assessment.Proposal.Validate(grant, project, action, time.Now().UTC()); err != nil {
		block("assessing", "Proposal rejected: "+boundedReason(err))
		return
	}
	if err := projectreg.ValidateBase(inspected, assessment.Proposal.BaseRevision); err != nil {
		block("assessing", "Proposal base is unauthorized")
		return
	}
	proposal, _, err := s.db.CreateLeadProposal(ctx, request.ID, record.ID, assessment.Proposal, "lead-proposal-"+request.ID)
	if err != nil {
		block("assessing", "Proposal could not be recorded")
		return
	}
	plan, planReplayed, err := s.db.FreezeLeadPlan(ctx, request.ID, proposal.ID, "lead-plan-"+request.ID)
	if err != nil {
		block("planned", "Plan freeze rejected: "+boundedReason(err))
		return
	}
	if !planReplayed {
		s.publishLeadEvent(ctx, domain.AggregateLeadRequest, request.ID, domain.EventLeadPlanFrozen)
	}
	spec, err := s.freezeLeadSpec(ctx, project, action, assessment.Proposal)
	if err != nil {
		block("frozen", "ActionSpec freeze rejected: "+boundedReason(err))
		return
	}
	if _, _, err := s.db.AdvanceLeadRequest(ctx, request.ID, "frozen", "frozen", store.LeadRequestLinks{
		SpecID: spec.ID,
	}); err != nil {
		block("frozen", "ActionSpec identity could not be linked")
		return
	}
	campaign, campaignReplayed, campaignEvents, err := s.db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: spec.ID, MaxRepairCycles: assessment.Proposal.MaxRepairCycles,
		MaxWallClockSecs:       assessment.Proposal.MaxWallClockSecs,
		MaxParallel:            assessment.Proposal.MaxParallel,
		MaxImplementerAttempts: assessment.Proposal.Budget.MaxAttempts * (assessment.Proposal.MaxRepairCycles + 1),
		IdempotencyKey:         "lead-campaign-" + request.ID,
	}, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		block("frozen", "Campaign creation rejected: "+boundedReason(err))
		return
	}
	if !campaignReplayed {
		for _, event := range campaignEvents {
			s.hub.Publish(event)
		}
	}
	_, _, err = s.db.AdvanceLeadRequest(ctx, request.ID, "frozen", "ready", store.LeadRequestLinks{
		FrozenPlanID: plan.ID, SpecID: spec.ID, CampaignID: campaign.ID,
	})
	if err != nil {
		block("frozen", "Lead projection could not be advanced")
	}
}

func (s *Server) publishLeadEvent(ctx context.Context, aggregateType, aggregateID, eventType string) {
	events, err := s.db.ListAggregateEvents(aggregateType, aggregateID)
	if err != nil {
		s.log.Warn("load Lead event for live publication failed", "aggregateId", aggregateID, "error", err.Error())
		return
	}
	for i := len(events) - 1; i >= 0; i-- {
		if events[i].EventType == eventType {
			s.leadEventMu.Lock()
			if s.leadEventIDs == nil {
				s.leadEventIDs = make(map[string]struct{})
			}
			if _, alreadyPublished := s.leadEventIDs[events[i].EventID]; alreadyPublished {
				s.leadEventMu.Unlock()
				return
			}
			s.leadEventIDs[events[i].EventID] = struct{}{}
			s.leadEventMu.Unlock()
			s.hub.Publish(events[i].Projected())
			return
		}
	}
}

func boundedReason(err error) string {
	message := err.Error()
	if len(message) > 600 {
		return message[:600]
	}
	return message
}

func boundedAssessmentFailure(err error) string {
	const marker = "reconciliation="
	message := err.Error()
	index := strings.Index(message, marker)
	if index < 0 {
		return "Lead assessment failed; no verified assessment was produced"
	}
	var outer struct {
		SessionReconciliation json.RawMessage `json:"sessionReconciliation"`
		WorkspaceArchive      string          `json:"workspaceArchive"`
	}
	if json.Unmarshal([]byte(message[index+len(marker):]), &outer) != nil || len(outer.SessionReconciliation) == 0 {
		return "Lead assessment failed; reconciliation evidence was malformed"
	}
	archiveStatus := "not_reported"
	if strings.HasPrefix(outer.WorkspaceArchive, "ok") {
		archiveStatus = "ok"
	} else if strings.HasPrefix(outer.WorkspaceArchive, "failed") {
		archiveStatus = "failed"
	}
	reason, marshalErr := json.Marshal(struct {
		Code                  string          `json:"code"`
		SessionReconciliation json.RawMessage `json:"sessionReconciliation"`
		WorkspaceArchive      string          `json:"workspaceArchive"`
	}{"assessment_run_failed", outer.SessionReconciliation, archiveStatus})
	if marshalErr != nil || len(reason)+len("Lead assessment failed; reconciliation=") > 600 {
		return "Lead assessment failed; reconciliation evidence exceeded its storage bound"
	}
	return "Lead assessment failed; reconciliation=" + string(reason)
}

func leadContext(ctx context.Context, repository, base string) (map[string]string, error) {
	cmd := exec.CommandContext(ctx, "git", "-C", repository, "ls-tree", "-r", "--name-only", base)
	out, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	if len(out) > 64_000 {
		out = out[:64_000]
	}
	files := strings.Split(string(out), "\n")
	if len(files) > 80 {
		files = files[:80]
	}
	var bounded strings.Builder
	for _, file := range files {
		if file == "" || bounded.Len()+len(file)+1 > 4096 {
			break
		}
		bounded.WriteString(file)
		bounded.WriteByte('\n')
	}
	if bounded.Len() == 0 {
		return map[string]string{"repository-files": "Repository tree has no listed files."}, nil
	}
	return map[string]string{"repository-files": bounded.String()}, nil
}

func (s *Server) freezeLeadSpec(ctx context.Context, project domain.Project, action domain.Action, plan domain.LeadPlanProposal) (domain.ActionSpec, error) {
	unit := plan.WorkUnits[0]
	inspected, err := projectreg.Inspect(ctx, project.RepositoryPath, project.BaseRefs)
	if err != nil {
		return domain.ActionSpec{}, err
	}
	if inspected.Identity != project.RepositoryIdentity {
		return domain.ActionSpec{}, fmt.Errorf("repository identity changed")
	}
	if err := projectreg.ValidateBase(inspected, plan.BaseRevision); err != nil {
		return domain.ActionSpec{}, err
	}
	capabilities := make([]string, len(unit.Capabilities))
	for i, capability := range unit.Capabilities {
		capabilities[i] = string(capability)
	}
	spec, err := (domain.NewActionSpecParams{
		ProjectID: project.ID, ActionID: action.ID, Goal: plan.Objective,
		Repository: project.RepositoryPath, BaseRevision: plan.BaseRevision,
		AllowedPaths: unit.AllowedPaths, AcceptanceCriteria: plan.AcceptanceCriteria,
		NegativeCases: plan.NegativeCases, RiskClass: string(plan.RiskClass),
		QualityProfile: string(plan.QualityProfile), Budget: plan.Budget,
		ExecutionMode: string(domain.ExecTrustedLocal), Provider: plan.Provider,
		Model: plan.Model, PaseoMode: plan.PaseoMode,
		ProjectConfigRevision: project.ConfigRevision, RepositoryIdentity: project.RepositoryIdentity,
		Capabilities: capabilities, BrowserEndpoints: project.BrowserEndpoints,
		BrowserTestServer: project.BrowserTestServer,
	}).Validate()
	if err != nil {
		return domain.ActionSpec{}, err
	}
	if missing := domain.MissingCapabilities(project.Capabilities, spec.Capabilities); len(missing) > 0 {
		return domain.ActionSpec{}, fmt.Errorf("project lacks proposed capabilities")
	}
	stored, _, _, err := s.db.CreateActionSpec(ctx, spec, telemetry.CorrelationIDFrom(ctx))
	return stored, err
}

func (s *Server) handleGetLeadRequest(w http.ResponseWriter, r *http.Request) {
	s.writeLeadRequest(w, r, r.PathValue("id"), true)
}

func (s *Server) handleLeadRequestEvents(w http.ResponseWriter, r *http.Request) {
	requestID := r.PathValue("id")
	if _, err := s.db.GetLeadRequest(r.Context(), requestID); err != nil {
		mapDomainError(w, err, s.log, "lead request events lookup")
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateLeadRequest, requestID)
	if err != nil {
		mapDomainError(w, err, s.log, "list lead request events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events)})
}

func (s *Server) writeLeadRequest(w http.ResponseWriter, r *http.Request, id string, replayed bool) {
	request, err := s.db.GetLeadRequest(r.Context(), id)
	if err != nil {
		mapDomainError(w, err, s.log, "lead request")
		return
	}
	projection := map[string]any{
		"id": request.ID, "projectId": request.ProjectID, "objective": request.Objective,
		"status": request.Status, "delegationId": request.DelegationID,
		"authorizationState": "denied", "actionId": request.ActionID,
		"actionSpecId": request.SpecID, "campaignId": request.CampaignID,
		"createdAt": request.CreatedAt, "updatedAt": request.UpdatedAt,
		"failureReason": request.FailureReason,
	}
	grant, gerr := s.db.GetStandingDelegation(r.Context(), request.DelegationID)
	project, perr := s.db.GetProject(request.ProjectID)
	if gerr == nil && perr == nil && grant.Validate(time.Now().UTC()) == nil && project.ConfigHash == request.ProjectConfigHash && project.Active() {
		projection["authorizationState"] = "permitted"
	}
	if request.AssessmentID != "" {
		if assessment, aerr := s.db.GetLeadAssessment(r.Context(), request.AssessmentID); aerr == nil {
			var body any
			_ = json.Unmarshal([]byte(assessment.AssessmentJSON), &body)
			projection["assessment"] = body
			var observation any
			_ = json.Unmarshal([]byte(assessment.RouteJSON), &observation)
			projection["modelObservation"] = observation
		}
	}
	if request.ProposalID != "" {
		if proposal, perr := s.db.GetLeadProposal(r.Context(), request.ProposalID); perr == nil {
			projection["plan"] = proposal.Proposal
		}
	}
	if request.FrozenPlanID != "" {
		if plan, ferr := s.db.GetFrozenLeadPlan(r.Context(), request.FrozenPlanID); ferr == nil {
			projection["planHash"] = plan.PlanHash
		}
	}
	if request.CampaignID != "" {
		if campaign, cerr := s.db.GetCampaign(request.CampaignID); cerr == nil {
			projection["campaignStatus"] = campaign.Status
			switch campaign.Status {
			case domain.CampaignAccepted:
				projection["status"] = "accepted"
			case domain.CampaignCancelled:
				projection["status"] = "cancelled"
			case domain.CampaignFailed, domain.CampaignBlocked, domain.CampaignHumanRequired:
				projection["status"] = "failed"
			}
		}
	}
	status := http.StatusOK
	if !replayed {
		status = http.StatusCreated
	}
	writeJSON(w, status, map[string]any{"request": projection})
}

func (s *Server) handleStartLeadRequest(w http.ResponseWriter, r *http.Request) {
	var body struct{}
	if !decodeBody(w, r, &body) {
		return
	}
	request, err := s.db.GetLeadRequest(r.Context(), r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "lead request")
		return
	}
	if request.Status != "ready" && request.Status != "running" {
		writeError(w, http.StatusConflict, "conflict", "lead request is not ready")
		return
	}
	applicable, authorized, err := s.db.CheckLeadCampaignAuthorization(r.Context(), request.CampaignID)
	if err != nil {
		mapDomainError(w, err, s.log, "lead authorization")
		return
	}
	if !applicable || !authorized {
		writeError(w, http.StatusForbidden, "unauthorized", "lead authority is stale or revoked")
		return
	}
	_, replayed, events, err := s.db.StartCampaign(r.Context(), request.CampaignID, telemetry.CorrelationIDFrom(r.Context()))
	if err != nil {
		mapDomainError(w, err, s.log, "start lead campaign")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	if !replayed && s.campaigns != nil {
		s.campaigns.Wake()
	}
	_, _, _ = s.db.AdvanceLeadRequest(r.Context(), request.ID, "ready", "running", store.LeadRequestLinks{})
	s.writeLeadRequest(w, r, request.ID, true)
}
