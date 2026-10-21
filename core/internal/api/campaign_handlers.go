package api

import (
	"context"
	"net/http"
	"strconv"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// CampaignCoordinator is the Core-owned scheduler wake/cancel boundary.
// The API never dispatches work directly; it records an authorized intent
// and wakes the deterministic engine.
type CampaignCoordinator interface {
	Wake()
	CancelCampaign(ctx context.Context, campaignID, reason, correlationID string) error
}

// WithCampaignCoordinator installs the native orchestrator. Without one the
// Core still records campaigns but cannot advance them; this is used only
// by isolated API tests, never by factory-core.
func (s *Server) WithCampaignCoordinator(coordinator CampaignCoordinator) *Server {
	s.campaigns = coordinator
	return s
}

type createCampaignRequest struct {
	SpecID           string `json:"specId"`
	MaxRepairCycles  *int   `json:"maxRepairCycles"`
	MaxWallClockSecs *int   `json:"maxWallClockSecs"`
	MaxParallel      *int   `json:"maxParallel"`
}

func (s *Server) handleCreateCampaign(w http.ResponseWriter, r *http.Request) {
	var req createCampaignRequest
	if !decodeBody(w, r, &req) {
		return
	}
	repairs := domain.DefaultCampaignRepair
	if req.MaxRepairCycles != nil {
		repairs = *req.MaxRepairCycles
	}
	params := domain.NewCampaignParams{
		SpecID:          req.SpecID,
		MaxRepairCycles: repairs, IdempotencyKey: strings.TrimSpace(r.Header.Get("Idempotency-Key")),
	}
	if req.MaxWallClockSecs != nil {
		params.MaxWallClockSecs = *req.MaxWallClockSecs
	}
	if req.MaxParallel != nil {
		params.MaxParallel = *req.MaxParallel
	}
	ctx := r.Context()
	campaign, replayed, events, err := s.db.CreateCampaign(ctx, params, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "create campaign")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		projection, perr := s.db.GetCampaignProjection(campaign.ID)
		if perr != nil {
			mapDomainError(w, perr, s.log, "campaign projection")
			return
		}
		writeJSON(w, http.StatusOK, projection)
		return
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	if s.campaigns != nil {
		s.campaigns.Wake()
	}
	s.log.Info("campaign created",
		"campaignId", campaign.ID, "actionId", campaign.ActionID,
		"specId", campaign.SpecID, "planHash", campaign.PlanHash,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	projection, err := s.db.GetCampaignProjection(campaign.ID)
	if err != nil {
		mapDomainError(w, err, s.log, "campaign projection")
		return
	}
	writeJSON(w, http.StatusCreated, projection)
}

func (s *Server) handleListCampaigns(w http.ResponseWriter, r *http.Request) {
	limit := 100
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 200 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be an integer 1..200")
			return
		}
		limit = n
	}
	var status domain.CampaignStatus
	if raw := strings.TrimSpace(r.URL.Query().Get("status")); raw != "" {
		status = domain.CampaignStatus(raw)
	}
	campaigns, err := s.db.ListCampaigns(strings.TrimSpace(r.URL.Query().Get("projectId")), status, limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list campaigns")
		return
	}
	if campaigns == nil {
		campaigns = []domain.Campaign{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"campaigns": campaigns})
}

func (s *Server) handleGetCampaign(w http.ResponseWriter, r *http.Request) {
	projection, err := s.db.GetCampaignProjection(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get campaign")
		return
	}
	writeJSON(w, http.StatusOK, projection)
}

func (s *Server) handleStartCampaign(w http.ResponseWriter, r *http.Request) {
	var body struct{}
	if !decodeBody(w, r, &body) {
		return
	}
	ctx := r.Context()
	campaign, replayed, events, err := s.db.StartCampaign(ctx, r.PathValue("id"), telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "start campaign")
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
	s.log.Info("campaign started", "campaignId", campaign.ID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	projection, err := s.db.GetCampaignProjection(campaign.ID)
	if err != nil {
		mapDomainError(w, err, s.log, "campaign projection")
		return
	}
	writeJSON(w, http.StatusOK, projection)
}

func (s *Server) handleCancelCampaign(w http.ResponseWriter, r *http.Request) {
	var body struct{}
	if !decodeBody(w, r, &body) {
		return
	}
	ctx := r.Context()
	campaignID := r.PathValue("id")
	var events []domain.Event
	var campaign domain.Campaign
	var err error
	if s.campaigns != nil {
		err = s.campaigns.CancelCampaign(ctx, campaignID, "operator cancelled", telemetry.CorrelationIDFrom(ctx))
		if err == nil {
			campaign, err = s.db.GetCampaign(campaignID)
		}
	} else {
		campaign, events, err = s.db.CancelCampaign(ctx, campaignID, "operator cancelled", telemetry.CorrelationIDFrom(ctx))
	}
	if err != nil {
		mapDomainError(w, err, s.log, "cancel campaign")
		return
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	s.log.Info("campaign cancelled", "campaignId", campaign.ID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	projection, err := s.db.GetCampaignProjection(campaign.ID)
	if err != nil {
		mapDomainError(w, err, s.log, "campaign projection")
		return
	}
	writeJSON(w, http.StatusOK, projection)
}

func (s *Server) handleCampaignEvents(w http.ResponseWriter, r *http.Request) {
	campaignID := r.PathValue("id")
	if _, err := s.db.GetCampaign(campaignID); err != nil {
		mapDomainError(w, err, s.log, "campaign events lookup")
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateCampaign, campaignID)
	if err != nil {
		mapDomainError(w, err, s.log, "list campaign events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events)})
}
