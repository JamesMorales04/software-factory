package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }

func mustDecode(raw []byte) map[string]any {
	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		return map[string]any{}
	}
	return m
}

// nextSequence allocates the next per-aggregate sequence inside the
// caller's transaction so concurrent writers cannot reuse a number.
func nextSequence(ctx context.Context, tx *sql.Tx, aggType, aggID string) (int64, error) {
	var max sql.NullInt64
	err := tx.QueryRowContext(ctx,
		`SELECT MAX(sequence) FROM events WHERE aggregate_type=? AND aggregate_id=?`,
		aggType, aggID).Scan(&max)
	if err != nil {
		return 0, err
	}
	if !max.Valid {
		return 1, nil
	}
	return max.Int64 + 1, nil
}

func insertEvent(ctx context.Context, tx *sql.Tx, e domain.Event) error {
	if err := e.Validate(); err != nil {
		return fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	payload, err := json.Marshal(e.Payload)
	if err != nil {
		return fmt.Errorf("marshal payload: %w", err)
	}
	_, err = tx.ExecContext(ctx,
		`INSERT INTO events(event_id, event_type, schema_version, aggregate_type, aggregate_id,
		 sequence, occurred_at, recorded_at, correlation_id, causation_id, actor, payload)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?)`,
		e.EventID, e.EventType, e.SchemaVersion, e.AggregateType, e.AggregateID,
		e.Sequence, stamp(e.OccurredAt), stamp(e.RecordedAt),
		e.CorrelationID, e.CausationID, e.Actor, string(payload),
	)
	if err != nil {
		return fmt.Errorf("insert event: %w", err)
	}
	return nil
}

func scanEvent(row interface {
	Scan(dest ...any) error
},
) (domain.Event, error) {
	var e domain.Event
	var occurred, recorded, payload string
	err := row.Scan(
		&e.GlobalSeq, &e.EventID, &e.EventType, &e.SchemaVersion,
		&e.AggregateType, &e.AggregateID, &e.Sequence,
		&occurred, &recorded, &e.CorrelationID, &e.CausationID, &e.Actor, &payload,
	)
	if err != nil {
		return domain.Event{}, err
	}
	e.OccurredAt, _ = time.Parse(time.RFC3339Nano, occurred)
	e.RecordedAt, _ = time.Parse(time.RFC3339Nano, recorded)
	e.Payload = mustDecode([]byte(payload))
	return e, nil
}

const eventColumns = `global_seq, event_id, event_type, schema_version, aggregate_type,
	aggregate_id, sequence, occurred_at, recorded_at, correlation_id, causation_id, actor, payload`

// ListEvents replays the global ledger in commit order starting after the
// given global sequence. Clients store the last seen globalSeq as their
// cursor; reconnecting with it yields exactly the missed suffix.
func (db *DB) ListEvents(after int64, limit int) ([]domain.Event, error) {
	if limit <= 0 || limit > 500 {
		limit = 100
	}
	rows, err := db.sql.Query(
		`SELECT `+eventColumns+` FROM events WHERE global_seq > ? ORDER BY global_seq ASC LIMIT ?`,
		after, limit,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Event
	for rows.Next() {
		e, err := scanEvent(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, e)
	}
	return out, rows.Err()
}

// ListAggregateEvents replays one aggregate's history in sequence order.
func (db *DB) ListAggregateEvents(aggType, aggID string) ([]domain.Event, error) {
	rows, err := db.sql.Query(
		`SELECT `+eventColumns+` FROM events WHERE aggregate_type=? AND aggregate_id=?
		 ORDER BY sequence ASC`, aggType, aggID,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Event
	for rows.Next() {
		e, err := scanEvent(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, e)
	}
	return out, rows.Err()
}

// CountEvents reports the ledger size (acceptance oracle).
func (db *DB) CountEvents() (int, error) {
	var n int
	return n, db.sql.QueryRow(`SELECT COUNT(*) FROM events`).Scan(&n)
}
