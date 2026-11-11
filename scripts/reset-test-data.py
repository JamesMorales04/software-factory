#!/usr/bin/env python3
"""Backup, reset and restore the disposable Factory test database.

This helper is normally called by ``scripts/reset-test-data.sh``. It is a
development/operator tool, not part of the installed Factory runtime, the Core
HTTP API or any automatic cleanup path. When invoked directly it independently
enforces the environment switch, project marker, exact confirmation, empty
evidence directory and ``/data`` refusal.

The reset deletes transactional rows while preserving the Project, execution
profile, provider, GitHub, charter, policy and owner-enrolment configuration
that make a test installation usable. Schema drift fails closed: every table
must be classified explicitly below, so a new migration cannot be silently
deleted or silently preserved.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
import stat
import sys
import tempfile
from datetime import UTC, datetime
from pathlib import Path

# Tables whose rows describe durable operator configuration or the authenticated
# owner's setup. They are never deleted by a test-data reset.
PRESERVED_TABLES = frozenset(
    {
        "_sqlx_migrations",
        "access_security_epoch",
        "agentic_owner_charters",
        "device_pairing_audit",
        "device_pairing_challenges",
        "device_session_audit",
        "device_sessions",
        "execution_policy_grants",
        "execution_profile_audit",
        "execution_profile_defaults",
        "execution_profile_revisions",
        "execution_profiles",
        "factory_settings",
        "github_connection_audit",
        "github_connections",
        "inference_ledger_policy",
        "issue_intake_policy_heads",
        "issue_intake_policy_revisions",
        "owner_bootstrap_claim",
        "project_configs",
        "project_discoveries",
        "project_github_bindings",
        "projects",
        "provider_settings",
        "provider_settings_audit",
        "skill_packs",
        "skill_refresh_state",
        "standing_delegations",
    }
)

# Every other current table is transactional, diagnostic or request-scoped.
# Keep this list explicit even though it is large: the union check below makes
# an unclassified future table a hard failure instead of an accidental delete.
TRANSACTIONAL_TABLES = frozenset(
    {
        "action_drafts",
        "action_specs",
        "actions",
        "agentic_assessments",
        "agentic_knowledge_packs",
        "agentic_operations",
        "agentic_owner_decisions",
        "agentic_participant_plans",
        "agentic_skill_adoptions",
        "agentic_skill_uses",
        "agentic_skills",
        "agentic_spec_issues",
        "agentic_spec_publications",
        "agentic_specs",
        "agentic_work_graphs",
        "architecture_plans",
        "attempt_specs",
        "attempts",
        "autonomy_repairs",
        "campaign_authorizations",
        "campaign_execution_bindings",
        "campaign_execution_profile_bindings",
        "campaign_progress_state",
        "campaign_review_requirements",
        "campaigns",
        "candidates",
        "definition_of_done",
        "delegation_usage",
        "dependency_plans",
        "documentation_plans",
        "events",
        "execution_check_receipts",
        "execution_policies",
        "external_effects",
        "finding_dispositions",
        "frozen_lead_plans",
        "gate_decision_reviewers",
        "gate_decision_revisions",
        "gate_decisions",
        "github_issue_page_etags",
        "github_issues",
        "github_sync_state",
        "idempotency",
        "idempotency_keys",
        "inference_authorizations",
        "inference_campaign_binding_quarantine",
        "inference_campaign_bindings",
        "inference_campaign_modes",
        "inference_envelopes",
        "inference_ledger_events",
        "inference_pricing_quotes",
        "inference_reservations",
        "intake_messages",
        "intake_requests",
        "issue_delivery_events",
        "issue_intake_audit",
        "issue_intake_batch_items",
        "issue_intake_batches",
        "issue_intake_queue",
        "issue_links",
        "lead_assessments",
        "lead_execution_context",
        "lead_integrations",
        "lead_proposals",
        "lead_requests",
        "lead_start_evidence",
        "lead_tasks",
        "lead_withdrawals",
        "model_call_reservations",
        "observability_cursor",
        "openspec_projections",
        "outbox",
        "plan_reviews",
        "planning_artifacts",
        "progress_checkpoints",
        "progress_decisions",
        "progress_evidence",
        "progress_holds",
        "progress_retired_work_items",
        "provider_sessions",
        "provider_tool_events",
        "provider_turn_egress_requests",
        "provider_turn_ledger_requests",
        "provider_turn_reservations",
        "quality_receipts",
        "repair_requests",
        "reviewer_plans",
        "runs",
        "skill_plans",
        "stack_decisions",
        "subscription_funding_policies",
        "subscription_scope_admissions",
        "worker_leases",
        "worker_receipts",
        "worker_sessions",
        "workflow_checkpoints",
        "workflow_runs",
        "workflow_steps",
    }
)

COUNT_TABLES = (
    "projects",
    "project_configs",
    "execution_profiles",
    "execution_profile_revisions",
    "execution_profile_defaults",
    "provider_settings",
    "github_connections",
    "project_github_bindings",
    "agentic_owner_charters",
    "standing_delegations",
    "factory_settings",
    "actions",
    "agentic_owner_decisions",
    "campaigns",
    "intake_requests",
    "lead_requests",
    "runs",
    "attempts",
    "candidates",
    "quality_receipts",
    "model_call_reservations",
    "inference_envelopes",
    "inference_reservations",
    "provider_sessions",
    "provider_turn_reservations",
    "events",
)

# A backup may contain only these exact names. Never derive a path from an
# arbitrary checksum entry.
ALLOWED_BACKUP_FILES = frozenset(
    {"factory.sqlite", "factory.sqlite-wal", "factory.sqlite-shm"}
)
FORBIDDEN_PROJECTS = frozenset({"software-factory", "production", "prod", "default"})
PROJECT_MARKER = re.compile(
    r"(^|[-_.])(test|tests|dev|development|e2e|uat|wp[0-9]+)([-_.]|$)"
)


class ResetError(RuntimeError):
    pass


class PostCommitFailure(ResetError):
    """The reset committed but the post-commit health/integrity check failed."""


def validate_guards(project: str, confirmed_project: str) -> None:
    """Enforce the same destructive-action guards for direct helper use."""
    if os.environ.get("FACTORY_TEST_DATA_RESET") != "1":
        raise ResetError(
            "set FACTORY_TEST_DATA_RESET=1 to acknowledge the destructive test-data reset"
        )
    if not project:
        raise ResetError("--project is required")
    if project in FORBIDDEN_PROJECTS:
        raise ResetError(f"project '{project}' is not a disposable test/dev project")
    if not PROJECT_MARKER.search(project):
        raise ResetError(
            f"project '{project}' must contain an explicit test/dev/e2e/uat/wp marker"
        )
    if confirmed_project != project:
        raise ResetError("--confirmed-project must exactly match --project")


def under_data(path: Path) -> bool:
    resolved = path.resolve(strict=False)
    return resolved == Path("/data") or Path("/data") in resolved.parents


def validate_paths(database: Path, evidence: Path, restore_from: Path | None) -> None:
    for label, path in (
        ("--database", database),
        ("--evidence-dir", evidence),
        ("--restore-from", restore_from),
    ):
        if path is not None and under_data(path):
            raise ResetError(f"{label} may not point inside the live /data volume")


def quote_ident(name: str) -> str:
    return '"' + name.replace('"', '""') + '"'


def utc_now() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def connect(path: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(f"file:{path}?mode=rw", uri=True, timeout=30)
    connection.isolation_level = None
    connection.execute("PRAGMA busy_timeout=30000")
    connection.execute("PRAGMA foreign_keys=ON")
    return connection


def schema_tables(connection: sqlite3.Connection) -> set[str]:
    rows = connection.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
    ).fetchall()
    return {row[0] for row in rows}


def validate_schema(connection: sqlite3.Connection) -> None:
    tables = schema_tables(connection)
    known = PRESERVED_TABLES | TRANSACTIONAL_TABLES
    unknown = sorted(tables - known)
    # `_sqlx_migrations` exists in a Core-owned database but not in a schema
    # assembled directly from the migration files by the shell test.
    required = known - {"_sqlx_migrations"}
    missing = sorted(required - tables)
    if unknown:
        raise ResetError(
            "unclassified database table(s): "
            + ", ".join(unknown)
            + ". Update scripts/reset-test-data.py before resetting."
        )
    if missing:
        raise ResetError(
            "expected table(s) are missing: "
            + ", ".join(missing)
            + ". The database schema is older or incomplete; refusing."
        )


def count_rows(connection: sqlite3.Connection) -> dict[str, int]:
    return {
        table: connection.execute(f"SELECT COUNT(*) FROM {quote_ident(table)}").fetchone()[0]
        for table in COUNT_TABLES
    }


def delete_order(connection: sqlite3.Connection) -> list[str]:
    """Return child-before-parent order for every transactional table."""
    parents: dict[str, set[str]] = {table: set() for table in TRANSACTIONAL_TABLES}
    for table in TRANSACTIONAL_TABLES:
        for row in connection.execute(f"PRAGMA foreign_key_list({quote_ident(table)})"):
            parent = row[2]
            if parent in TRANSACTIONAL_TABLES and parent != table:
                parents[table].add(parent)
    referenced_by: dict[str, set[str]] = {table: set() for table in TRANSACTIONAL_TABLES}
    for child, table_parents in parents.items():
        for parent in table_parents:
            referenced_by[parent].add(child)
    remaining = set(TRANSACTIONAL_TABLES)
    ordered: list[str] = []
    while remaining:
        leaves = sorted(table for table in remaining if not (referenced_by[table] & remaining))
        if not leaves:
            # The current schema has no delete cycle. Keep a deterministic tail
            # and rely on deferred FK checks instead of silently dropping the
            # remaining tables.
            ordered.extend(sorted(remaining))
            break
        ordered.extend(leaves)
        remaining.difference_update(leaves)
    return ordered


def validate_regular_file(path: Path, label: str, required: bool) -> None:
    """Reject symlinks/non-regular files so a backup has a stable target."""
    if path.is_symlink():
        raise ResetError(f"{label} may not be a symlink: {path}")
    if not path.exists():
        if required:
            raise ResetError(f"{label} does not exist: {path}")
        return
    if not stat.S_ISREG(path.stat().st_mode):
        raise ResetError(f"{label} must be a regular file: {path}")


def validate_database_inputs(database: Path, required: bool) -> None:
    validate_regular_file(database, "database", required)
    for suffix in ("-wal", "-shm"):
        validate_regular_file(
            Path(str(database) + suffix),
            f"database sidecar {suffix}",
            required=False,
        )


def sha256_file(path: Path) -> tuple[str, int]:
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    digest = hashlib.sha256()
    size = 0
    with os.fdopen(descriptor, "rb") as handle:
        if not stat.S_ISREG(os.fstat(handle.fileno()).st_mode):
            raise ResetError(f"refusing to hash a non-regular file: {path}")
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


def copy_regular_file(
    source: Path,
    destination: Path,
    source_label: str = "backup source",
    destination_label: str = "backup destination",
) -> None:
    """Copy one regular file to a new regular file without following links."""
    validate_regular_file(source, source_label, required=True)
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        if not stat.S_ISREG(os.fstat(source_fd).st_mode):
            raise ResetError(f"{source_label} is not a regular file: {source}")
        source_handle = os.fdopen(source_fd, "rb")
        source_fd = -1
        with source_handle:
            destination_fd = os.open(
                destination,
                os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                0o600,
            )
            with os.fdopen(destination_fd, "wb") as destination_handle:
                shutil.copyfileobj(source_handle, destination_handle)
    finally:
        if source_fd != -1:
            os.close(source_fd)
    if destination.is_symlink() or not stat.S_ISREG(destination.lstat().st_mode):
        raise ResetError(f"{destination_label} is not a regular file: {destination}")
    os.chmod(destination, 0o600)


def copy_and_hash(source: Path, destination: Path) -> dict[str, object]:
    copy_regular_file(source, destination)
    digest, size = sha256_file(destination)
    return {"sha256": digest, "bytes": size}


def restore_destination(database: Path, name: str) -> Path:
    if name == "factory.sqlite":
        return database
    if name == "factory.sqlite-wal":
        return Path(str(database) + "-wal")
    if name == "factory.sqlite-shm":
        return Path(str(database) + "-shm")
    raise ResetError(f"unexpected backup file: {name}")


def restore_temp_path(destination: Path) -> Path:
    return destination.with_name(destination.name + ".restore-tmp")


def safe_unlink(path: Path) -> None:
    """Remove one path without following a symlink."""
    if path.is_symlink():
        os.unlink(path)
        return
    if not path.exists():
        return
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ResetError(f"refusing to remove a non-regular path: {path}")
    os.unlink(path)


def preflight_restore_temps(temp_paths: list[Path]) -> None:
    """Reject or safely clear every destination temp path before mutation."""
    for temporary in temp_paths:
        if temporary.is_symlink():
            raise ResetError(f"restore temporary path may not be a symlink: {temporary}")
        if temporary.exists():
            if not stat.S_ISREG(temporary.lstat().st_mode):
                raise ResetError(
                    f"restore temporary path must be a regular file: {temporary}"
                )
            os.unlink(temporary)


def cleanup_restore_temps(temp_paths: list[Path]) -> None:
    for temporary in temp_paths:
        if temporary.is_symlink() or temporary.exists():
            try:
                os.unlink(temporary)
            except OSError:
                pass


def rollback_restore(database: Path, evidence: Path, had_database: bool) -> None:
    """Restore the pre-restore backup after a failed restore mutation."""
    if not had_database:
        for suffix in ("", "-wal", "-shm"):
            safe_unlink(Path(str(database) + suffix))
        return
    entries = verify_backup(evidence)
    staging = Path(tempfile.mkdtemp(prefix=".factory-rollback-", dir=database.parent))
    try:
        staged: list[tuple[Path, Path]] = []
        destinations: set[Path] = set()
        for name in entries:
            source = safe_backup_path(evidence, name)
            destination = restore_destination(database, name)
            temporary = staging / name
            copy_regular_file(source, temporary, "pre-restore backup", "rollback file")
            staged.append((temporary, destination))
            destinations.add(destination)
        for suffix in ("-wal", "-shm"):
            stale = Path(str(database) + suffix)
            if stale not in destinations:
                safe_unlink(stale)
        for temporary, destination in staged:
            os.replace(temporary, destination)
            if destination.is_symlink() or not stat.S_ISREG(destination.lstat().st_mode):
                raise ResetError(f"rollback destination is not a regular file: {destination}")
        for name, destination in (
            (name, restore_destination(database, name)) for name in entries
        ):
            digest, _size = sha256_file(destination)
            if digest != entries[name]["sha256"]:
                raise ResetError(f"rollback checksum mismatch: {name}")
    finally:
        shutil.rmtree(staging, ignore_errors=True)


def write_json(path: Path, payload: dict[str, object]) -> None:
    serialized = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w") as handle:
            handle.write(serialized)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
    except Exception:
        if temporary.is_symlink() or temporary.exists():
            try:
                os.unlink(temporary)
            except OSError:
                pass
        raise
    if path.is_symlink() or not stat.S_ISREG(path.lstat().st_mode):
        try:
            os.unlink(path)
        except OSError:
            pass
        raise ResetError(f"JSON report destination is not a regular file: {path}")


def ensure_empty_directory(path: Path) -> None:
    if path.is_symlink():
        raise ResetError(f"evidence directory may not be a symlink: {path}")
    path.mkdir(parents=True, exist_ok=True)
    if any(path.iterdir()):
        raise ResetError(f"evidence directory must be empty: {path}")


def backup_database(
    database: Path,
    evidence: Path,
    project: str,
    mode: str,
    before: dict[str, int],
    include_shm: bool = False,
) -> Path:
    validate_database_inputs(database, required=True)
    ensure_empty_directory(evidence)
    suffixes = ("", "-wal", "-shm") if include_shm else ("", "-wal")
    files: dict[str, dict[str, object]] = {}
    for suffix in suffixes:
        source = Path(str(database) + suffix)
        if not source.exists():
            continue
        name = "factory.sqlite" + suffix
        files[name] = copy_and_hash(source, evidence / name)
    if "factory.sqlite" not in files:
        raise ResetError(f"database file is missing: {database}")
    manifest = {
        "kind": "factory-test-data-reset",
        "version": 1,
        "created_at": utc_now(),
        "project": project,
        "source_database": str(database),
        "mode": mode,
        "files": files,
        "counts_before": before,
        "restore": (
            "Stop the Compose project, verify SHA256SUMS, then run "
            "scripts/reset-test-data.sh --restore-from this-directory. "
            "Raw SQL deletion is not a Factory product feature."
        ),
    }
    write_json(evidence / "manifest.json", manifest)
    sums = "".join(f"{entry['sha256']}  {name}\n" for name, entry in sorted(files.items()))
    sums_path = evidence / "SHA256SUMS"
    sums_path.write_text(sums)
    os.chmod(sums_path, 0o600)
    return evidence


def safe_backup_path(backup: Path, name: str) -> Path:
    """Resolve one exact allowlisted backup file without path traversal."""
    if name not in ALLOWED_BACKUP_FILES:
        raise ResetError(f"invalid backup file name: {name!r}")
    if (
        name in {".", ".."}
        or Path(name).is_absolute()
        or Path(name).name != name
        or "/" in name
        or "\\" in name
    ):
        raise ResetError(f"unsafe backup file name: {name!r}")
    candidate = backup / name
    if candidate.is_symlink():
        raise ResetError(f"backup file may not be a symlink: {name}")
    if not candidate.is_file():
        raise ResetError(f"backup file is missing: {candidate}")
    candidate_resolved = candidate.resolve(strict=True)
    backup_resolved = backup.resolve(strict=True)
    if candidate_resolved.parent != backup_resolved:
        raise ResetError(f"backup file escapes the evidence directory: {name}")
    return candidate


def verify_backup(backup: Path) -> dict[str, dict[str, object]]:
    manifest_path = backup / "manifest.json"
    sums_path = backup / "SHA256SUMS"
    if manifest_path.is_symlink() or sums_path.is_symlink():
        raise ResetError("backup manifest and SHA256SUMS may not be symlinks")
    if not manifest_path.is_file() or not sums_path.is_file():
        raise ResetError(f"backup is missing manifest.json or SHA256SUMS: {backup}")
    if sums_path.stat().st_size > 64 * 1024:
        raise ResetError("SHA256SUMS is unexpectedly large")
    entries: dict[str, dict[str, object]] = {}
    for line in sums_path.read_text().splitlines():
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2 or len(parts[0]) != 64:
            raise ResetError(f"invalid SHA256SUMS line: {line!r}")
        digest, name = parts
        path = safe_backup_path(backup, name)
        if name in entries:
            raise ResetError(f"duplicate backup file entry: {name}")
        actual, size = sha256_file(path)
        if actual != digest:
            raise ResetError(f"backup checksum mismatch: {name}")
        entries[name] = {"sha256": actual, "bytes": size}
    if "factory.sqlite" not in entries:
        raise ResetError("backup does not contain factory.sqlite")
    manifest = json.loads(manifest_path.read_text())
    if not isinstance(manifest, dict) or manifest.get("kind") != "factory-test-data-reset":
        raise ResetError("backup manifest is not a Factory test-data backup")
    manifest_files = manifest.get("files")
    if not isinstance(manifest_files, dict) or any(
        name not in ALLOWED_BACKUP_FILES for name in manifest_files
    ):
        raise ResetError("backup manifest contains an invalid file name")
    return entries


def drop_delete_triggers(connection: sqlite3.Connection) -> list[tuple[str, str]]:
    rows = connection.execute(
        "SELECT name,sql FROM sqlite_master "
        "WHERE type='trigger' AND (upper(sql) LIKE '%BEFORE DELETE%' "
        "OR upper(sql) LIKE '%INSTEAD OF DELETE%') ORDER BY name"
    ).fetchall()
    triggers: list[tuple[str, str]] = []
    for name, sql in rows:
        if not sql:
            raise ResetError(f"trigger has no definition: {name}")
        connection.execute(f"DROP TRIGGER {quote_ident(name)}")
        triggers.append((name, sql))
    return triggers


def restore_delete_triggers(
    connection: sqlite3.Connection, triggers: list[tuple[str, str]]
) -> None:
    for name, sql in triggers:
        connection.execute(sql)
        present = connection.execute(
            "SELECT 1 FROM sqlite_master WHERE type='trigger' AND name=?", (name,)
        ).fetchone()
        if present is None:
            raise ResetError(f"failed to restore trigger: {name}")


def trigger_names(connection: sqlite3.Connection) -> set[str]:
    return {
        row[0]
        for row in connection.execute(
            "SELECT name FROM sqlite_master WHERE type='trigger'"
        ).fetchall()
    }


def integrity_ok(connection: sqlite3.Connection) -> bool:
    row = connection.execute("PRAGMA integrity_check").fetchone()
    return bool(row) and row[0] == "ok"


def apply_reset(connection: sqlite3.Connection, dry_run: bool) -> dict[str, int]:
    order = delete_order(connection)
    before_triggers = trigger_names(connection)
    connection.execute("BEGIN IMMEDIATE")
    try:
        connection.execute("PRAGMA defer_foreign_keys=ON")
        triggers = drop_delete_triggers(connection)
        for table in order:
            connection.execute(f"DELETE FROM {quote_ident(table)}")
        violations = connection.execute("PRAGMA foreign_key_check").fetchall()
        if violations:
            raise ResetError(f"foreign key violations after reset: {violations[:5]}")
        after = count_rows(connection)
        if dry_run:
            connection.execute("ROLLBACK")
            if trigger_names(connection) != before_triggers:
                raise ResetError("dry-run did not restore the delete triggers")
            return after
        restore_delete_triggers(connection, triggers)
        # Check the whole transactional result before making it durable.
        if not integrity_ok(connection):
            raise ResetError("pre-commit integrity check failed")
        connection.execute("COMMIT")
    except Exception:
        if connection.in_transaction:
            connection.execute("ROLLBACK")
        raise
    # A post-commit failure is recoverable only by restoring the verified
    # pre-reset backup. The test-only fault injection exercises that path.
    if os.environ.get("FACTORY_TEST_DATA_RESET_FAIL_AFTER_COMMIT") == "1":
        raise PostCommitFailure("injected post-commit integrity check failure")
    if not integrity_ok(connection):
        raise PostCommitFailure("post-commit integrity check failed")
    return after


def restore_database(
    database: Path,
    backup: Path,
    evidence: Path,
    project: str,
) -> None:
    entries = verify_backup(backup)
    validate_database_inputs(database, required=False)
    database.parent.mkdir(parents=True, exist_ok=True)
    ensure_empty_directory(evidence)
    had_database = database.exists()
    if had_database:
        backup_database(
            database, evidence, project, "pre-restore", {}, include_shm=True
        )
        # The rollback source must be verified before any destination mutation.
        verify_backup(evidence)

    mappings = [
        (safe_backup_path(backup, name), restore_destination(database, name))
        for name in entries
    ]
    temp_paths = [restore_temp_path(destination) for _, destination in mappings]
    # All temp paths are checked before a sidecar is deleted or a file replaced.
    preflight_restore_temps(temp_paths)
    try:
        for (source, _destination), temporary in zip(mappings, temp_paths):
            copy_regular_file(source, temporary, "backup file", "restore temporary file")
    except Exception:
        cleanup_restore_temps(temp_paths)
        raise

    destinations = {destination for _, destination in mappings}
    stale_paths = [
        Path(str(database) + suffix)
        for suffix in ("-wal", "-shm")
        if Path(str(database) + suffix) not in destinations
    ]
    mutated = False
    try:
        for stale in stale_paths:
            safe_unlink(stale)
            mutated = True
        for (_source, destination), temporary in zip(mappings, temp_paths):
            os.replace(temporary, destination)
            mutated = True
            if destination.is_symlink() or not stat.S_ISREG(destination.lstat().st_mode):
                raise ResetError(f"restore destination is not a regular file: {destination}")
            if (
                os.environ.get(
                    "FACTORY_TEST_DATA_RESET_FAIL_RESTORE_AFTER_REPLACE"
                )
                == "1"
            ):
                raise ResetError("injected restore failure after replacement")
        connection = connect(database)
        try:
            integrity = connection.execute("PRAGMA integrity_check").fetchone()
            if not integrity or integrity[0] != "ok":
                raise ResetError(f"restored database integrity check failed: {integrity}")
            violations = connection.execute("PRAGMA foreign_key_check").fetchall()
            if violations:
                raise ResetError(
                    f"restored database has foreign key violations: {violations[:5]}"
                )
        finally:
            connection.close()
    except Exception as error:
        if mutated:
            try:
                rollback_restore(database, evidence, had_database)
            except Exception as rollback_error:
                raise ResetError(
                    f"restore failed and rollback failed: {error}; {rollback_error}; "
                    f"pre-restore evidence: {evidence}"
                ) from error
            raise ResetError(f"restore failed; original database restored: {error}") from error
        raise
    finally:
        cleanup_restore_temps(temp_paths)


def chown_paths(database: Path, owner: tuple[int, int] | None) -> None:
    if owner is None:
        return
    if os.geteuid() != 0:
        raise ResetError("--chown requires a root helper container")
    uid, gid = owner
    for suffix in ("", "-wal", "-shm"):
        path = Path(str(database) + suffix)
        if path.exists():
            os.chown(path, uid, gid)


def best_effort_chown(database: Path, owner: tuple[int, int] | None) -> None:
    try:
        chown_paths(database, owner)
    except Exception:
        pass


def parse_owner(value: str) -> tuple[int, int]:
    try:
        uid, gid = value.split(":", 1)
        return int(uid), int(gid)
    except (ValueError, AttributeError) as error:
        raise argparse.ArgumentTypeError("expected UID:GID") from error


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--evidence-dir", required=True, type=Path)
    parser.add_argument("--project", required=True)
    parser.add_argument("--confirmed-project", required=True)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--restore-from", type=Path)
    parser.add_argument("--chown", type=parse_owner)
    args = parser.parse_args(argv)

    validate_guards(args.project, args.confirmed_project)
    validate_paths(args.database, args.evidence_dir, args.restore_from)

    if args.dry_run and args.restore_from:
        raise ResetError("--dry-run and --restore-from are mutually exclusive")
    if args.restore_from:
        validate_database_inputs(args.database, required=False)
    else:
        validate_database_inputs(args.database, required=True)

    if args.restore_from:
        try:
            restore_database(
                args.database, args.restore_from, args.evidence_dir, args.project
            )
        except Exception:
            best_effort_chown(args.database, args.chown)
            raise
        chown_paths(args.database, args.chown)
        print(f"restored: {args.database} from {args.restore_from}")
        print("integrity: ok")
        return 0

    connection = connect(args.database)
    post_commit_failure: PostCommitFailure | None = None
    try:
        validate_schema(connection)
        before = count_rows(connection)
        backup_database(
            args.database,
            args.evidence_dir,
            args.project,
            "dry-run" if args.dry_run else "reset",
            before,
        )
        try:
            after = apply_reset(connection, args.dry_run)
        except PostCommitFailure as failure:
            post_commit_failure = failure
    finally:
        connection.close()

    if post_commit_failure is not None:
        recovery_evidence = args.evidence_dir / "recovery-before-restore"
        try:
            restore_database(
                args.database,
                args.evidence_dir,
                recovery_evidence,
                args.project,
            )
            chown_paths(args.database, args.chown)
            write_json(
                args.evidence_dir / "recovery-report.json",
                {
                    "outcome": "RESTORED_PRE_RESET",
                    "failure": str(post_commit_failure),
                    "created_at": utc_now(),
                    "pre_restore_backup": str(recovery_evidence),
                },
            )
        except Exception as recovery_error:
            best_effort_chown(args.database, args.chown)
            write_json(
                args.evidence_dir / "recovery-report.json",
                {
                    "outcome": "RECOVERY_FAILED",
                    "failure": str(post_commit_failure),
                    "recovery_error": str(recovery_error),
                    "created_at": utc_now(),
                },
            )
            raise ResetError(
                f"{post_commit_failure}; automatic recovery failed: {recovery_error}"
            ) from recovery_error
        raise ResetError(
            f"{post_commit_failure}; pre-reset backup restored from {args.evidence_dir}"
        )

    chown_paths(args.database, args.chown)

    report = {
        "result": "DRY_RUN" if args.dry_run else "RESET",
        "database_modified": not args.dry_run,
        "project": args.project,
        "created_at": utc_now(),
        "before": before,
        "after": after,
        "preserved_tables": sorted(PRESERVED_TABLES),
        "transactional_tables": sorted(TRANSACTIONAL_TABLES),
    }
    write_json(args.evidence_dir / "reset-report.json", report)

    print(f"backup: {args.evidence_dir}")
    print(f"database: {args.database}")
    print(f"mode: {'DRY_RUN' if args.dry_run else 'RESET'}")
    for table in COUNT_TABLES:
        print(f"count {table}: {before[table]} -> {after[table]}")
    if args.dry_run:
        print("database unchanged: dry-run rolled back")
    else:
        print("database reset: committed and integrity checked")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except ResetError as error:
        print(f"reset-test-data: {error}", file=sys.stderr)
        raise SystemExit(1) from error
