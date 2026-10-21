package api

import (
	"net/http"
	"path/filepath"
	"strconv"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	projectreg "github.com/JamesMorales04/software-factory/core/internal/project"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

type registerProjectRequest struct {
	ID               string   `json:"id"`
	Name             string   `json:"name"`
	RepositoryPath   string   `json:"repositoryPath"`
	BaseRefs         []string `json:"baseRefs"`
	QualityProfile   string   `json:"qualityProfile"`
	WorkspaceScope   string   `json:"workspaceScope"`
	Capabilities     []string `json:"capabilities"`
	BrowserEndpoints []string `json:"browserEndpoints"`
	ExecutionMode    string   `json:"executionMode"`
}

func (s *Server) handleListProjects(w http.ResponseWriter, r *http.Request) {
	projects, err := s.db.ListProjects()
	if err != nil {
		mapDomainError(w, err, s.log, "list projects")
		return
	}
	if projects == nil {
		projects = []domain.Project{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"projects": projects})
}

func (s *Server) handleGetProject(w http.ResponseWriter, r *http.Request) {
	project, err := s.db.GetProject(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get project")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"project": project})
}

func (s *Server) handleRegisterProject(w http.ResponseWriter, r *http.Request) {
	var req registerProjectRequest
	if !decodeBody(w, r, &req) {
		return
	}
	ctx := r.Context()
	repository, err := projectreg.Inspect(ctx, req.RepositoryPath, req.BaseRefs)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", "repository registration rejected: "+err.Error())
		return
	}
	params := domain.NewProjectParams{
		ID: req.ID, Name: req.Name, RepositoryPath: repository.Path,
		RepositoryIdentity: repository.Identity, BaseRefs: req.BaseRefs,
		QualityProfile: req.QualityProfile, WorkspaceScope: req.WorkspaceScope,
		Capabilities: req.Capabilities, BrowserEndpoints: req.BrowserEndpoints,
		ExecutionMode: req.ExecutionMode,
	}
	project, err := params.Validate()
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	stored, event, err := s.db.CreateProject(ctx, project, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "register project")
		return
	}
	s.hub.Publish(event)
	s.log.Info("project registered", "projectId", stored.ID,
		"repositoryIdentity", stored.RepositoryIdentity,
		"configRevision", stored.ConfigRevision,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"project": stored})
}

type updateProjectRequest struct {
	Name             *string   `json:"name"`
	Status           *string   `json:"status"`
	BaseRefs         *[]string `json:"baseRefs"`
	QualityProfile   *string   `json:"qualityProfile"`
	WorkspaceScope   *string   `json:"workspaceScope"`
	Capabilities     *[]string `json:"capabilities"`
	BrowserEndpoints *[]string `json:"browserEndpoints"`
	RepositoryPath   *string   `json:"repositoryPath"`
}

func (s *Server) handleUpdateProject(w http.ResponseWriter, r *http.Request) {
	var req updateProjectRequest
	if !decodeBody(w, r, &req) {
		return
	}
	ctx := r.Context()
	current, err := s.db.GetProject(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "update project lookup")
		return
	}
	if !current.Registered() {
		writeError(w, http.StatusConflict, "conflict", "legacy projects cannot be reconfigured")
		return
	}
	update := domain.ProjectUpdateParams{
		Name: req.Name, Status: req.Status, BaseRefs: req.BaseRefs,
		QualityProfile: req.QualityProfile, WorkspaceScope: req.WorkspaceScope,
		Capabilities: req.Capabilities, BrowserEndpoints: req.BrowserEndpoints,
		RepositoryPath: req.RepositoryPath,
	}
	baseRefs := current.BaseRefs
	if req.BaseRefs != nil {
		baseRefs = *req.BaseRefs
	}
	if req.RepositoryPath != nil || req.BaseRefs != nil {
		path := current.RepositoryPath
		if req.RepositoryPath != nil {
			path = *req.RepositoryPath
		}
		repository, ierr := projectreg.Inspect(ctx, path, baseRefs)
		if ierr != nil {
			writeError(w, http.StatusBadRequest, "invalid", "project update rejected: "+ierr.Error())
			return
		}
		if repository.Identity != current.RepositoryIdentity {
			writeError(w, http.StatusConflict, "conflict", "repository identity is immutable; register a new project instead")
			return
		}
		if req.RepositoryPath != nil && filepath.Clean(repository.Path) != filepath.Clean(current.RepositoryPath) {
			canonical := repository.Path
			update.RepositoryPath = &canonical
		}
	}
	stored, event, changed, err := s.db.UpdateProject(ctx, current.ID, update, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "update project")
		return
	}
	if !changed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"project": stored})
		return
	}
	s.hub.Publish(event)
	s.log.Info("project updated", "projectId", stored.ID,
		"configRevision", stored.ConfigRevision, "status", stored.Status,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusOK, map[string]any{"project": stored})
}

func (s *Server) handleProjectRefs(w http.ResponseWriter, r *http.Request) {
	project, err := s.db.GetProject(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "project refs lookup")
		return
	}
	if !project.Registered() {
		writeError(w, http.StatusConflict, "conflict", "legacy projects have no repository base-ref policy")
		return
	}
	repository, err := projectreg.Inspect(r.Context(), project.RepositoryPath, project.BaseRefs)
	if err != nil {
		writeError(w, http.StatusConflict, "conflict", "registered repository is unavailable: "+err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"repositoryIdentity": repository.Identity,
		"repositoryPath":     repository.Path,
		"baseRefs":           repository.BaseTips,
	})
}

func (s *Server) handleProjectEvents(w http.ResponseWriter, r *http.Request) {
	if _, err := s.db.GetProject(r.PathValue("id")); err != nil {
		mapDomainError(w, err, s.log, "project events lookup")
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateProject, r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list project events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events)})
}

// projectConfigRevision exposes the immutable stored revision for audit.
func (s *Server) handleProjectConfigRevision(w http.ResponseWriter, r *http.Request) {
	revision, err := strconv.Atoi(r.URL.Query().Get("revision"))
	if err != nil || revision < 1 {
		writeError(w, http.StatusBadRequest, "invalid", "revision must be a positive integer")
		return
	}
	snapshot, err := s.db.ProjectConfigRevision(r.PathValue("id"), revision)
	if err != nil {
		mapDomainError(w, err, s.log, "project config revision")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"config": snapshot})
}
