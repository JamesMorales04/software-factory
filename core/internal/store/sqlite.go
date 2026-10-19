// Package store owns durable state: Projects, Actions and the domain
// event ledger backed by SQLite in WAL mode. Business mutations and
// their events commit in a single transaction; the ledger is the
// replayable source of truth for the UI.
package store

import (
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"fmt"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	_ "modernc.org/sqlite"
)

// SQLite pragmas are fixed at open time: WAL for crash-safe concurrent
// readers, synchronous NORMAL (durable with WAL), foreign keys enforced
// and a bounded busy timeout instead of indefinite blocking.
const openParams = "?_pragma=journal_mode(WAL)&_pragma=synchronous(NORMAL)&_pragma=foreign_keys(ON)&_pragma=busy_timeout(5000)"

const schema = `
CREATE TABLE IF NOT EXISTS projects (
	id TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS actions (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	title TEXT NOT NULL,
	description TEXT NOT NULL DEFAULT '',
	status TEXT NOT NULL,
	idempotency_key TEXT UNIQUE,
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
	global_seq INTEGER PRIMARY KEY AUTOINCREMENT,
	event_id TEXT NOT NULL UNIQUE,
	event_type TEXT NOT NULL,
	schema_version TEXT NOT NULL,
	aggregate_type TEXT NOT NULL,
	aggregate_id TEXT NOT NULL,
	sequence INTEGER NOT NULL,
	occurred_at TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	correlation_id TEXT NOT NULL DEFAULT '',
	causation_id TEXT NOT NULL DEFAULT '',
	actor TEXT NOT NULL DEFAULT '',
	payload TEXT NOT NULL DEFAULT '{}',
	UNIQUE (aggregate_type, aggregate_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_events_global ON events(global_seq);
CREATE INDEX IF NOT EXISTS idx_actions_project ON actions(project_id, created_at);
`

// DB is the S1 persistence adapter. A single logical writer owns
// transactions; readers observe WAL snapshots.
type DB struct {
	sql *sql.DB
}

// Open creates the schema, enforces WAL mode and seeds the default project.
func Open(path string) (*DB, error) {
	sqldb, err := sql.Open("sqlite", path+openParams)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	sqldb.SetMaxOpenConns(1)
	if _, err := sqldb.Exec(schema); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("apply schema: %w", err)
	}
	db := &DB{sql: sqldb}
	if err := db.assertWAL(); err != nil {
		_ = sqldb.Close()
		return nil, err
	}
	if err := db.seedDefaultProject(); err != nil {
		_ = sqldb.Close()
		return nil, err
	}
	return db, nil
}

// Close releases the database handle.
func (db *DB) Close() error { return db.sql.Close() }

// assertWAL verifies the durability assumption S1 depends on instead of
// assuming the pragma took effect.
func (db *DB) assertWAL() error {
	var mode string
	if err := db.sql.QueryRow(`PRAGMA journal_mode`).Scan(&mode); err != nil {
		return fmt.Errorf("read journal_mode: %w", err)
	}
	if !strings.EqualFold(mode, "wal") {
		return fmt.Errorf("journal_mode is %q, S1 requires WAL", mode)
	}
	return nil
}

// JournalMode reports the active journal mode (used by acceptance tests).
func (db *DB) JournalMode() (string, error) {
	var mode string
	if err := db.sql.QueryRow(`PRAGMA journal_mode`).Scan(&mode); err != nil {
		return "", err
	}
	return mode, nil
}

func newID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return hex.EncodeToString(b[:])
}

func utcNow() time.Time { return time.Now().UTC().Truncate(time.Millisecond) }

func (db *DB) seedDefaultProject() error {
	_, err := db.sql.Exec(
		`INSERT INTO projects(id, name, created_at) VALUES(?,?,?)
		 ON CONFLICT(id) DO NOTHING`,
		domain.DefaultProjectID, "Default Project", utcNow().Format(time.RFC3339Nano),
	)
	return err
}
