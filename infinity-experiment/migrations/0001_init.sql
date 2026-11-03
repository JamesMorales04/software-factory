PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS threads (
    thread_id TEXT PRIMARY KEY,
    parent_thread_id TEXT,
    root_thread_id TEXT NOT NULL,
    spawn_message_order INTEGER,
    spawn_tool_call_id TEXT,
    closed INTEGER NOT NULL DEFAULT 0,
    is_subscription_event INTEGER NOT NULL DEFAULT 0,
    is_compaction INTEGER NOT NULL DEFAULT 0,
    fresh_context INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS threads_parent ON threads(parent_thread_id);
CREATE INDEX IF NOT EXISTS threads_root ON threads(root_thread_id);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    message_json TEXT NOT NULL,
    dedup_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(thread_id, ordinal),
    UNIQUE(thread_id, dedup_id)
);

CREATE INDEX IF NOT EXISTS messages_thread_order ON messages(thread_id, ordinal);

CREATE TABLE IF NOT EXISTS compaction_summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    up_to_order INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS compaction_thread ON compaction_summaries(thread_id, id);

CREATE TABLE IF NOT EXISTS processed_ids (
    thread_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    PRIMARY KEY(thread_id, message_id)
);

CREATE TABLE IF NOT EXISTS thread_metadata (
    thread_id TEXT PRIMARY KEY,
    metadata_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subscriptions (
    thread_id TEXT NOT NULL,
    tool_call_id TEXT NOT NULL,
    PRIMARY KEY(thread_id, tool_call_id)
);

CREATE TABLE IF NOT EXISTS pending_choices (
    thread_id TEXT NOT NULL,
    choice_id TEXT NOT NULL,
    choice_json TEXT NOT NULL,
    PRIMARY KEY(thread_id, choice_id)
);

CREATE TABLE IF NOT EXISTS thread_stops (
    thread_id TEXT PRIMARY KEY,
    stopped INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
    objective TEXT NOT NULL,
    status TEXT NOT NULL,
    root_thread_id TEXT NOT NULL,
    candidate_sha256 TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE IF NOT EXISTS agent_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_key TEXT NOT NULL UNIQUE,
    run_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS agent_events_run ON agent_events(run_id, seq);

CREATE TABLE IF NOT EXISTS effects (
    effect_key TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    request_json TEXT NOT NULL,
    state TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    lease_token TEXT,
    marker_path TEXT,
    marker_sha256 TEXT,
    result_json TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS effects_run ON effects(run_id, state);

CREATE TABLE IF NOT EXISTS callbacks (
    callback_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    effect_key TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    tool_call_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    delivered_at TEXT,
    UNIQUE(effect_key, attempt)
);

CREATE INDEX IF NOT EXISTS callbacks_pending ON callbacks(status, run_id);

CREATE TABLE IF NOT EXISTS candidates (
    run_id TEXT PRIMARY KEY,
    candidate_sha256 TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    review_json TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
