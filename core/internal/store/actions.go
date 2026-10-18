package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// ActorLocalOperator attributes S1 writes made through the local API token.
const ActorLocalOperator = "local-operator"

// ListProjects returns the seeded S1 projects ordered by id.
func (db *DB) ListProjects() ([]domain.Project, error) {
	rows, err := db.sql.Query(`SELECT id, name, created_at FROM projects ORDER BY id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Project
	for rows.Next() {
		var p domain.Project
		var ts string
		if err := rows.Scan(&p.ID, &p.Name, &ts); err != nil {
			return nil, err
		}
		p.CreatedAt, _ = time.Parse(time.RFC3339Nano, ts)
		out = append(out, p)
	}
	return out, rows.Err()
}

// CreateAction persists an Action and its ActionCreated event atomically.
// When idempotencyKey matches an existing row, the original Action is
// returned with replayed=true and no new event is recorded.
func (db *DB) CreateAction(ctx context.Context, params domain.NewActionParams, correlationID string) (domain.Action, bool, error) {
	now := utcNow()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Action{}, false, err
	}
	defer func() { _ = tx.Rollback() }()

	var projectExists bool
	if err := tx.QueryRowContext(ctx, `SELECT TRUE FROM projects WHERE id=?`, params.ProjectID).Scan(&projectExists); err != nil {
		if err == sql.ErrNoRows {
			return domain.Action{}, false, fmt.Errorf("%w: unknown projectId %q", domain.ErrInvalidInput, params.ProjectID)
		}
		return domain.Action{}, false, err
	}

	action := domain.Action{
		ID:             newID(),
		ProjectID:      params.ProjectID,
		Title:          params.Title,
		Description:    params.Description,
		Status:         domain.StatusPending,
		IdempotencyKey: params.IdempotencyKey,
		CreatedAt:      now,
		UpdatedAt:      now,
	}

	var key any
	if action.IdempotencyKey != "" {
		key = action.IdempotencyKey
	}
	_, err = tx.ExecContext(ctx,
		`INSERT INTO actions(id, project_id, title, description, status, idempotency_key, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?)`,
		action.ID, action.ProjectID, action.Title, action.Description,
		string(action.Status), key, stamp(now), stamp(now),
	)
	if err != nil {
		if params.IdempotencyKey != "" {
			if existing, rerr := findByIdempotencyKey(ctx, tx, params.IdempotencyKey); rerr == nil {
				// Idempotent replay: commit nothing new, return the original.
				if cerr := tx.Commit(); cerr != nil {
					return domain.Action{}, false, cerr
				}
				return existing, true, nil
			}
		}
		return domain.Action{}, false, fmt.Errorf("insert action: %w", err)
	}

	payload, _ := json.Marshal(map[string]any{
		"projectId":   action.ProjectID,
		"title":       action.Title,
		"description": action.Description,
		"status":      string(action.Status),
	})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventActionCreated,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateAction,
		AggregateID:   action.ID,
		Sequence:      1,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         ActorLocalOperator,
		Payload:       mustDecode(payload),
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Action{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Action{}, false, err
	}
	actionWithSeq, err := db.GetAction(action.ID)
	if err != nil {
		return domain.Action{}, false, err
	}
	return actionWithSeq, false, nil
}

func findByIdempotencyKey(ctx context.Context, tx *sql.Tx, key string) (domain.Action, error) {
	var a domain.Action
	var status, created, updated string
	var desc sql.NullString
	err := tx.QueryRowContext(ctx,
		`SELECT id, project_id, title, description, status, created_at, updated_at
		 FROM actions WHERE idempotency_key=?`, key,
	).Scan(&a.ID, &a.ProjectID, &a.Title, &desc, &status, &created, &updated)
	if err != nil {
		return domain.Action{}, err
	}
	a.Description = desc.String
	a.Status = domain.ActionStatus(status)
	a.IdempotencyKey = key
	a.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	a.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return a, nil
}

// GetAction returns one Action by id or domain.ErrNotFound.
func (db *DB) GetAction(id string) (domain.Action, error) {
	var a domain.Action
	var status, created, updated string
	var desc, key sql.NullString
	err := db.sql.QueryRow(
		`SELECT id, project_id, title, description, status, idempotency_key, created_at, updated_at
		 FROM actions WHERE id=?`, id,
	).Scan(&a.ID, &a.ProjectID, &a.Title, &desc, &status, &key, &created, &updated)
	if err == sql.ErrNoRows {
		return domain.Action{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.Action{}, err
	}
	a.Description = desc.String
	a.Status = domain.ActionStatus(status)
	a.IdempotencyKey = key.String
	a.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	a.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return a, nil
}

// ListActions returns Actions newest first, optionally filtered.
func (db *DB) ListActions(projectID string, status domain.ActionStatus, limit int) ([]domain.Action, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	q := `SELECT id, project_id, title, description, status, idempotency_key, created_at, updated_at
	      FROM actions WHERE 1=1`
	var args []any
	if projectID != "" {
		q += ` AND project_id=?`
		args = append(args, projectID)
	}
	if status != "" {
		q += ` AND status=?`
		args = append(args, string(status))
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ?`
	args = append(args, limit)
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Action
	for rows.Next() {
		var a domain.Action
		var st, created, updated string
		var desc, key sql.NullString
		if err := rows.Scan(&a.ID, &a.ProjectID, &a.Title, &desc, &st, &key, &created, &updated); err != nil {
			return nil, err
		}
		a.Description = desc.String
		a.Status = domain.ActionStatus(st)
		a.IdempotencyKey = key.String
		a.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
		a.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
		out = append(out, a)
	}
	return out, rows.Err()
}

// CountActions reports how many Actions exist (acceptance oracle).
func (db *DB) CountActions() (int, error) {
	var n int
	return n, db.sql.QueryRow(`SELECT COUNT(*) FROM actions`).Scan(&n)
}

// TransitionAction moves an Action along one legal edge and records an
// ActionTransitioned event in the same transaction.
func (db *DB) TransitionAction(ctx context.Context, id string, to domain.ActionStatus, correlationID string) (domain.Action, domain.Event, error) {
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	var a domain.Action
	var status, created, updated string
	var desc, key sql.NullString
	err = tx.QueryRowContext(ctx,
		`SELECT id, project_id, title, description, status, idempotency_key, created_at, updated_at
		 FROM actions WHERE id=?`, id,
	).Scan(&a.ID, &a.ProjectID, &a.Title, &desc, &status, &key, &created, &updated)
	if err == sql.ErrNoRows {
		return domain.Action{}, domain.Event{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	a.Description = desc.String
	a.Status = domain.ActionStatus(status)
	a.IdempotencyKey = key.String
	a.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)

	if !a.CanTransition(to) {
		return domain.Action{}, domain.Event{}, fmt.Errorf("%w: %s -> %s", domain.ErrInvalidTransition, a.Status, to)
	}
	now := utcNow()
	if _, err := tx.ExecContext(ctx, `UPDATE actions SET status=?, updated_at=? WHERE id=?`,
		string(to), stamp(now), id); err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	a.Status = to
	a.UpdatedAt = now

	payload, _ := json.Marshal(map[string]any{"from": status, "to": string(to)})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventActionTransitioned,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateAction,
		AggregateID:   id,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         ActorLocalOperator,
		Payload:       mustDecode(payload),
	}
	event.Sequence, err = nextSequence(ctx, tx, domain.AggregateAction, id)
	if err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Action{}, domain.Event{}, err
	}
	return a, event, nil
}

// RecordCoreStarted appends a CoreStarted boot marker so restarts are
// visible in the replayable timeline.
func (db *DB) RecordCoreStarted(ctx context.Context, version string) (domain.Event, error) {
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()
	now := utcNow()
	payload, _ := json.Marshal(map[string]any{"version": version})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventCoreStarted,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateCore,
		AggregateID:   AggregateCoreID,
		OccurredAt:    now,
		RecordedAt:    now,
		Actor:         ActorLocalOperator,
		Payload:       mustDecode(payload),
	}
	var errSeq error
	event.Sequence, errSeq = nextSequence(ctx, tx, domain.AggregateCore, AggregateCoreID)
	if errSeq != nil {
		return domain.Event{}, errSeq
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

// AggregateCoreID is the singleton aggregate for Core lifecycle events.
const AggregateCoreID = "core"
