package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// S4-C Project Registry persistence. Project state is Core-authoritative;
// configuration revisions are immutable snapshots so a frozen ActionSpec
// can always be interpreted against the exact configuration it recorded.

const projectColumns = `id, name, registration_status, status, repository_path,
	repository_identity, base_refs, quality_profile, workspace_scope,
	capabilities, browser_endpoints, browser_test_server, execution_mode,
	security_label, config_revision, config_hash, registered_at, created_at,
	updated_at`

func scanProject(row interface{ Scan(dest ...any) error }) (domain.Project, error) {
	var p domain.Project
	var registration, status, baseRefs, profile, caps, endpoints, testServer string
	var registered, created, updated string
	if err := row.Scan(
		&p.ID, &p.Name, &registration, &status, &p.RepositoryPath,
		&p.RepositoryIdentity, &baseRefs, &profile, &p.WorkspaceScope,
		&caps, &endpoints, &testServer, &p.ExecutionMode, &p.SecurityLabel,
		&p.ConfigRevision, &p.ConfigHash, &registered, &created, &updated,
	); err != nil {
		return domain.Project{}, err
	}
	p.RegistrationStatus = domain.ProjectRegistrationStatus(registration)
	p.Status = domain.ProjectStatus(status)
	p.QualityProfile = domain.QualityProfile(profile)
	if err := json.Unmarshal([]byte(baseRefs), &p.BaseRefs); err != nil {
		return domain.Project{}, err
	}
	var capabilityValues []string
	if err := json.Unmarshal([]byte(caps), &capabilityValues); err != nil {
		return domain.Project{}, err
	}
	p.Capabilities = make([]domain.Capability, 0, len(capabilityValues))
	for _, value := range capabilityValues {
		p.Capabilities = append(p.Capabilities, domain.Capability(value))
	}
	if err := json.Unmarshal([]byte(endpoints), &p.BrowserEndpoints); err != nil {
		return domain.Project{}, err
	}
	// Project normalization represents an absent browser endpoint list as nil.
	// The database encoder stores both nil and empty slices as `[]`, so restore
	// that canonical representation before validating the configuration hash.
	if len(p.BrowserEndpoints) == 0 {
		p.BrowserEndpoints = nil
	}
	if testServer != "" && testServer != "null" {
		var server domain.BrowserTestServer
		if err := json.Unmarshal([]byte(testServer), &server); err != nil {
			return domain.Project{}, err
		}
		p.BrowserTestServer = &server
	}
	p.RegisteredAt = parseStamp(registered)
	p.CreatedAt = parseStamp(created)
	p.UpdatedAt = parseStamp(updated)
	return p, nil
}

// CreateProject registers a validated project atomically with its first
// immutable configuration revision and audit event.
func (db *DB) CreateProject(ctx context.Context, project domain.Project, correlationID string) (domain.Project, domain.Event, error) {
	if err := project.Validate(); err != nil {
		return domain.Project{}, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	if !project.Registered() {
		return domain.Project{}, domain.Event{}, fmt.Errorf("%w: only registered projects can be created", domain.ErrInvalidInput)
	}
	now := db.now()
	project.CreatedAt = now
	project.UpdatedAt = now
	project.RegisteredAt = now
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Project{}, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO projects(id, name, registration_status, status, repository_path,
		 repository_identity, base_refs, quality_profile, workspace_scope,
		 capabilities, browser_endpoints, browser_test_server, execution_mode,
		 security_label, config_revision, config_hash, registered_at,
		 last_dispatched_at, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'',?,?)`,
		project.ID, project.Name, string(project.RegistrationStatus), string(project.Status),
		project.RepositoryPath, project.RepositoryIdentity, encodeStrings(project.BaseRefs),
		string(project.QualityProfile), project.WorkspaceScope,
		encodeStrings(capabilityStrings(project.Capabilities)),
		encodeStrings(project.BrowserEndpoints), encodeBrowserTestServer(project.BrowserTestServer),
		project.ExecutionMode, project.SecurityLabel, project.ConfigRevision, project.ConfigHash,
		stamp(now), stamp(now), stamp(now),
	); err != nil {
		if isUniqueViolation(err) {
			return domain.Project{}, domain.Event{}, fmt.Errorf("%w: project id or repository identity is already registered", domain.ErrConflict)
		}
		return domain.Project{}, domain.Event{}, fmt.Errorf("insert project: %w", err)
	}
	if err := insertProjectRevisionTx(ctx, tx, project, now); err != nil {
		return domain.Project{}, domain.Event{}, err
	}
	payload := projectEventPayload(project)
	event, err := appendProjectEvent(ctx, tx, domain.EventProjectRegistered, project.ID, correlationID, now, payload)
	if err != nil {
		return domain.Project{}, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Project{}, domain.Event{}, err
	}
	stored, err := db.GetProject(project.ID)
	if err != nil {
		return domain.Project{}, domain.Event{}, err
	}
	return stored, event, nil
}

// UpdateProject applies an operator configuration change. Repository
// identity is immutable; path changes are rejected once specs exist, and a
// capability required by any frozen spec cannot be revoked.
func (db *DB) UpdateProject(ctx context.Context, id string, update domain.ProjectUpdateParams, correlationID string) (domain.Project, domain.Event, bool, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	current, err := scanProject(tx.QueryRowContext(ctx,
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.Project{}, domain.Event{}, false, domain.ErrNotFound
	}
	if err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	next, changed, err := update.Apply(current)
	if err != nil {
		return domain.Project{}, domain.Event{}, false, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	if !changed {
		if err := tx.Commit(); err != nil {
			return domain.Project{}, domain.Event{}, false, err
		}
		return current, domain.Event{}, false, nil
	}
	specs, err := listProjectSpecsTx(ctx, tx, id)
	if err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	if next.RepositoryPath != current.RepositoryPath && len(specs) > 0 {
		return domain.Project{}, domain.Event{}, false, fmt.Errorf("%w: repository path cannot change while frozen specs exist", domain.ErrConflict)
	}
	for _, spec := range specs {
		if missing := domain.MissingCapabilities(next.Capabilities, spec.Capabilities); len(missing) > 0 {
			return domain.Project{}, domain.Event{}, false, fmt.Errorf(
				"%w: capability %v is required by frozen spec %s", domain.ErrConflict, missing, spec.ID)
		}
	}
	next.UpdatedAt = now
	if _, err := tx.ExecContext(ctx,
		`UPDATE projects SET name=?, status=?, repository_path=?, base_refs=?,
		 quality_profile=?, workspace_scope=?, capabilities=?, browser_endpoints=?,
		 browser_test_server=?, execution_mode=?, security_label=?,
		 config_revision=?, config_hash=?, updated_at=? WHERE id=?`,
		next.Name, string(next.Status), next.RepositoryPath, encodeStrings(next.BaseRefs),
		string(next.QualityProfile), next.WorkspaceScope,
		encodeStrings(capabilityStrings(next.Capabilities)),
		encodeStrings(next.BrowserEndpoints), encodeBrowserTestServer(next.BrowserTestServer),
		next.ExecutionMode, next.SecurityLabel, next.ConfigRevision, next.ConfigHash,
		stamp(now), id,
	); err != nil {
		return domain.Project{}, domain.Event{}, false, fmt.Errorf("update project: %w", err)
	}
	if err := insertProjectRevisionTx(ctx, tx, next, now); err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	eventType := domain.EventProjectUpdated
	if current.Status != next.Status {
		eventType = domain.EventProjectStatusChanged
	}
	payload := projectEventPayload(next)
	payload["previousRevision"] = current.ConfigRevision
	event, err := appendProjectEvent(ctx, tx, eventType, id, correlationID, now, payload)
	if err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	stored, err := db.GetProject(id)
	if err != nil {
		return domain.Project{}, domain.Event{}, false, err
	}
	return stored, event, true, nil
}

// GetProject returns one project or domain.ErrNotFound.
func (db *DB) GetProject(id string) (domain.Project, error) {
	project, err := scanProject(db.sql.QueryRow(
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.Project{}, domain.ErrNotFound
	}
	return project, err
}

// ListProjects returns all projects ordered by id.
func (db *DB) ListProjects() ([]domain.Project, error) {
	rows, err := db.sql.Query(`SELECT ` + projectColumns + ` FROM projects ORDER BY id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Project
	for rows.Next() {
		project, err := scanProject(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, project)
	}
	return out, rows.Err()
}

// ProjectConfigRevision returns the immutable stored snapshot for audit.
func (db *DB) ProjectConfigRevision(projectID string, revision int) (map[string]any, error) {
	var raw string
	if err := db.sql.QueryRow(
		`SELECT config FROM project_config_revisions WHERE project_id=? AND revision=?`,
		projectID, revision).Scan(&raw); err != nil {
		if err == sql.ErrNoRows {
			return nil, domain.ErrNotFound
		}
		return nil, err
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return nil, err
	}
	return out, nil
}

func insertProjectRevisionTx(ctx context.Context, tx *sql.Tx, project domain.Project, now time.Time) error {
	raw, err := json.Marshal(project)
	if err != nil {
		return err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO project_config_revisions(project_id, revision, config_hash, config, created_at)
		 VALUES(?,?,?,?,?)`,
		project.ID, project.ConfigRevision, project.ConfigHash, string(raw), stamp(now),
	); err != nil {
		return fmt.Errorf("insert project config revision: %w", err)
	}
	return nil
}

func listProjectSpecsTx(ctx context.Context, tx *sql.Tx, projectID string) ([]domain.ActionSpec, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE project_id=? ORDER BY created_at ASC`, projectID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.ActionSpec
	for rows.Next() {
		spec, err := scanSpec(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, spec)
	}
	return out, rows.Err()
}

func encodeBrowserTestServer(server *domain.BrowserTestServer) string {
	if server == nil {
		return "null"
	}
	raw, err := json.Marshal(server)
	if err != nil {
		return "null"
	}
	return string(raw)
}

func projectEventPayload(project domain.Project) map[string]any {
	return map[string]any{
		"projectId":          project.ID,
		"name":               project.Name,
		"registrationStatus": string(project.RegistrationStatus),
		"status":             string(project.Status),
		"repositoryIdentity": project.RepositoryIdentity,
		"qualityProfile":     string(project.QualityProfile),
		"workspaceScope":     project.WorkspaceScope,
		"capabilities":       capabilityStrings(project.Capabilities),
		"browserEndpoints":   project.BrowserEndpoints,
		"browserTestServer":  project.BrowserTestServer,
		"configRevision":     project.ConfigRevision,
		"configHash":         project.ConfigHash,
	}
}

func appendProjectEvent(ctx context.Context, tx *sql.Tx, eventType, projectID, correlationID string, now time.Time, payload map[string]any) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateProject,
		AggregateID:   projectID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         ActorLocalOperator,
		Payload:       payload,
	}
	sequence, err := nextSequence(ctx, tx, domain.AggregateProject, projectID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = sequence
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

func encodeStrings(values []string) string {
	if values == nil {
		return "[]"
	}
	raw, err := json.Marshal(values)
	if err != nil {
		return "[]"
	}
	return string(raw)
}

func capabilityStrings(caps []domain.Capability) []string {
	out := make([]string, len(caps))
	for i, capability := range caps {
		out[i] = string(capability)
	}
	return out
}

func isUniqueViolation(err error) bool {
	return err != nil && strings.Contains(strings.ToLower(err.Error()), "unique constraint")
}
