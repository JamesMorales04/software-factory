# SQLx and migrations reference

## Queries

- Prefer `sqlx::query!` / `query_as!` for new or changed SQL so the schema and
  types are checked at compile time.
- If a runtime query is unavoidable, keep it parameterized and add a schema
  integration test.
- Never build SQL with string interpolation from untrusted input.
- Bind values; do not quote them into SQL.
- Classify unique/foreign-key/constraint/busy/not-found errors explicitly.
- Bound result sizes and use `fetch_optional`/`fetch_one` deliberately.

## Transactions

- Use one transaction for a multi-row invariant.
- Acquire the earliest write lock deliberately; do not hold a transaction across
  an external call.
- Use `BEGIN IMMEDIATE` where the write contention model requires it.
- Retry only safely repeatable operations; never retry an unknown external
  effect.
- Commit only after the durable invariant is satisfied; propagate commit errors.

## SQLite/WAL

- Enable WAL and foreign keys; set a bounded busy timeout.
- WAL still serializes writers; a larger pool does not increase write throughput.
- Close connections cleanly; do not leave a transaction open on cancellation.
- Backup/restore must be tested; a copied WAL without its `-wal`/`-shm` is not a
  database backup.

## Migrations

- Migrations are ordered, immutable and tested from a clean database.
- A destructive migration is R2/R3 and needs an explicit owner decision.
- Add columns/tables first; backfill; switch reads; remove later.
- Never rewrite a released migration.
- A rollback plan is required for a destructive or irreversible change.
- Verify the schema after migration, not only that the SQL exited zero.

## Offline/CI

- If using compile-time query macros, run `cargo sqlx prepare --check` in CI and
  commit the offline metadata.
- Do not make the installed product require a live database at build time.
- Keep SQLx tooling versions pinned with the workspace.

## Evidence

Record migration IDs, clean-database result, schema assertions, busy/locked
negative cases, backup/restore result and any `NOT_TESTED`.
