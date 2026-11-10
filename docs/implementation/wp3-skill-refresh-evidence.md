# WP3 — skill-pack refresh scheduler and broader technology catalog

**Status:** implementation evidence only — **not** acceptance, certification or a
completion claim. No paid model inference was run. The stale/missing skill gate
was not weakened.

- Branch: `factory/quality-wp3-skill-refresh`
- Implementation commit: `ba2e2c99d0b289f59b86a763f492244e31fcee03`
- Baseline: `bcb8b5b`
- Design authority:
  [ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md)
- Delivery plan:
  [quality-first-delivery-plan.md](quality-first-delivery-plan.md) (WP3)

## Review-finding repairs

The first independent review at `c91ea78` found five valid issues; commit
`cbf45fd` repaired them (pip pack, Docker-specific aliases, durable rotating
project cursor, Office states, and an initial source manifest). The second
review at `cf8f890` found two remaining issues, repaired here:

1. **HTTP availability was still used as freshness.** `SkillPack` now separates
   `source_checked_at` from `content_reviewed_at` and carries an explicit
   `review_basis` (`SOURCE_AVAILABILITY_ONLY` or `CONTENT_REVIEWED`),
   `review_scope` and `reviewed_sections`. The classifier treats only
   content-reviewed packs as fresh. Source-only packs validate as source
   metadata but are always stale/blocking and cannot be persisted by
   `Database::save_skill_pack`.
2. **Office labeled the next project window as already scanned.** The panel now
   labels status counts as the “Next scan window” and shows the completed pass
   from `lastOutcome` separately.

### Content-review basis

The catalog has 68 pack entries including three compatibility aliases. Fifteen
packs are content-reviewed against named official source sections on
2026-11-09:

`rust`, `cargo`, `axum`, `sqlx`, `tokio`, `typescript`, `react`, `vite`, `npm`,
`python`, `pip`, `django`, `flask`, `pytest`, `ruff`.

The other canonical packs and the `docker`/`docker-compose`/`podman` aliases are
`SOURCE_AVAILABILITY_ONLY`: their cited URLs were checked, but their content was
not reviewed. They remain blocking rather than claiming freshness. This is the
explicit option (b) from the review: unreviewed packs stay fail-closed.

[skill-pack catalog review evidence](../evidence/skill-pack-catalog-2026-11-09/review.json)
records, per pack:

- `sourceCheckedAt` and HTTP status for every cited source URL;
- `contentReview` with `reviewedAt`, `reviewAfter`, scope and the exact source
  section headings checked, or `null` for source-only packs.

On 2026-11-09 all 74 unique source URLs returned HTTP 200 (HTTP GET, redirects
followed). A test verifies that the manifest matches every pack’s source list,
source-check date, review basis, content-review date, scope and section list.

## What changed

### Deterministic refresh policy

`native/src/planning/skills.rs` classifies each required technology from the
latest persisted revision and the curated builtin catalog:

| State | Meaning | Blocks planning |
|---|---|---|
| `FRESH` | A persisted revision or builtin is content-reviewed, valid and before `review_after`. | No |
| `DUE` | A content-reviewed persisted revision is past `review_after` and no newer, different content-reviewed builtin exists. | Yes |
| `REFRESHABLE` | A content-reviewed persisted revision is due and the builtin has a later `content_reviewed_at` and a different content hash. | No; a refresh can append an immutable revision. |
| `STALE` | The builtin is past `review_after`, invalid, or only source-availability-checked; no usable persisted revision exists. | Yes |
| `MISSING` | Neither a persisted revision nor a builtin pack exists. | Yes |

`build_skill_plan` uses the same classification. Unknown technologies,
source-only builtins, stale builtins, invalid packs and due packs without a safe
builtin remain fail-closed. Review dates are never extended in place, and no
source URL is invented.

### Core scheduler and API

`native/src/planning/skill_refresh.rs` adds one Core-owned background task:

- runs one pass immediately, then sleeps for the configured interval;
- interval env `FACTORY_SKILL_REFRESH_INTERVAL_SECONDS`, default `86400`
  seconds, minimum `3600`, maximum `604800`; an invalid value fails Core
  startup instead of silently clamping;
- one task only, with `watch`-based shutdown and an idempotent
  `shutdown_scheduler()` awaited before the database closes;
- serializes passes with a process-wide async mutex;
- scans at most 64 registered project roots per pass using the durable rotating
  cursor in `skill_refresh_state`, caps status output at 256 technologies and
  caps event detail at 700 characters;
- reports checked/skipped/total projects in every status and report so a
  bounded pass never implies full coverage;
- refreshes only a due persisted revision when the builtin is content-reviewed,
  newer and different; it does not pre-seed the whole catalog;
- records durable idempotent events through `Database::record_event_once`
  (`skill.pack.refresh_due`, `skill.pack.refreshed`,
  `skill.pack.refresh_blocked`);
- logs one bounded summary per pass, not one line per pack.

Operator-authenticated endpoints:

- `GET /agentic/skills/refresh` — current classification, due/blocked counts,
  interval, project window, last/next run, last outcome and bounded status list.
- `POST /agentic/skills/refresh` — explicit deterministic refresh pass.

A blocked `FORMAL_SPEC` plan also records one idempotent
`skill.pack.refresh_blocked` event on the request while preserving the existing
`SKILL_PACK_REQUIRED` response. The scheduler never approves a plan, hides a
stale pack, lowers a lane or grants authority.

### Broader stack detection and catalog

Stack detection in `native/src/agentic.rs` recognizes Java/Gradle/Maven,
Kotlin, C#/.NET/NuGet, Ruby/Bundler/Rails, PHP/Composer/Laravel, Swift,
Vue/Svelte/Angular, Django/Flask, PostgreSQL/MySQL/Redis,
Kubernetes/Terraform/GitHub Actions, pnpm/yarn/bun/Deno, Tauri/Electron and the
Rust `actix-web`/`rocket`/`serde`/`tracing`/`reqwest`/`hyper`/`tonic` libraries.
Detection remains bounded to root manifests and at most 512 root entries; it
does not recurse.

The curated catalog moved to
`native/src/planning/skills/catalog.rs` and now has 65 canonical entries plus
three self-named compatibility aliases. The previously added
`actix-web`, `angular`, `bun`, `bundler`, `composer`, `csharp`, `deno`,
`diesel`, `django`, `dotnet`, `electron`, `flask`, `github-actions`, `gradle`,
`hyper`, `java`, `kotlin`, `kubernetes`, `laravel`, `maven`, `mysql`, `nuget`,
`php`, `pip`, `pnpm`, `postgresql`, `rails`, `redis`, `reqwest`, `rocket`,
`ruby`, `serde`, `svelte`, `swift`, `tauri`, `terraform`, `tonic`, `tracing`,
`vue` and `yarn` packs remain present. Docker aliases use Docker-specific
sources and commands.

Every pack validates its source list, source-check date, required best
practices, anti-patterns, security notes and commands. Content-reviewed packs
also require a valid review window, review scope and reviewed source sections.

### Office

The existing Core-backed Skills route has a “Technology knowledge packs” panel:

- shows interval, last/next pass, due/refreshable count, blocking count, the
  **next scan window** and the last completed pass;
- lists only non-fresh packs with state badges, reason and review deadline when
  a content review exists;
- exposes an explicit “Run refresh pass” action that calls the authenticated
  Core POST;
- adds English and Spanish strings and component tests for loading, load
  failure, action failure, due/blocked states, truncated technologies, deferred
  projects, next-window labeling and last-pass counts.

The panel does not invent state or offer a decorative action. The agentic skill
registry remains separate and unchanged in authority.

## Changed files

- `native/src/agentic.rs`
- `native/src/bin/core.rs`
- `native/src/db/mod.rs`
- `native/src/db/planning.rs`
- `native/src/db/projects.rs`
- `native/src/planning/mod.rs`
- `native/src/planning/skill_refresh.rs`
- `native/src/planning/skills.rs`
- `native/src/planning/skills/catalog.rs`
- `migrations/0065_skill_refresh_cursor.sql`
- `migrations/0066_skill_pack_content_review.sql`
- `native/tests/migration_skill_refresh.rs`
- `office/src/coreApi.ts`
- `office/src/i18n/catalog.en.ts`
- `office/src/i18n/catalog.es.ts`
- `office/src/views/Skills.test.tsx`
- `office/src/views/Skills.tsx`
- `office/src/views/knowledge.css`
- `docs/evidence/skill-pack-catalog-2026-11-09/review.json`
- `docs/product/office-parity.json` (CAP-038 only)
- `docs/implementation/quality-first-planning-status.md` (WP3 row)
- `docs/implementation/wp3-skill-refresh-evidence.md` (this file)

## Verification run

From `native/`:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib planning::skills
9 passed; 0 failed

cargo test -p factory-native --lib planning::skill_refresh
6 passed; 0 failed

cargo test -p factory-native --lib
744 passed; 0 failed; 2 ignored

cargo test -p factory-native --test migration_skill_refresh
1 passed; 0 failed

cargo test -p factory-native --test planning_pipeline
1 passed; 0 failed

cargo test -p factory-native --bin factory-core agentic_plan_and_spec_routes_require_operator_credentials
1 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

The two ignored library tests are pre-existing Podman-dependent fixtures; no new
test is ignored.

From `office/`:

```text
npm test -- --run
303 passed across 45 files

npm run typecheck
clean

npm run lint
clean (161 files)
```

From the repository root:

```text
podman compose config
exit 0
```

Covered tests include FRESH/DUE/REFRESHABLE/STALE/MISSING classification,
source-only packs never being fresh, immutable refresh revision, idempotent
scheduler pass and events, scheduler shutdown, missing-technology fail-closed
behavior, Python/pip planning, detected-stack catalog coverage, catalog
source/content-review manifest integrity, Docker source correctness, durable
project rotation with skipped counts, migration `0065` cursor durability and
`0066` historical-row preservation, endpoint authentication/bounds and the
Office loading/error/action/truncation/deferred/next-window states.

## Limitations and NOT_TESTED

- **NOT_TESTED:** a real Compose Core restart with the scheduler active, the
  scheduler interval boundary in a running container, migrations on a populated
  Compose volume, and durable event inspection through the Compose stack. The
  migration upgrade path is covered by a local pre-change database test; only
  `podman compose config` was run for Compose.
- **NOT_TESTED:** real browser acceptance of the Skills technology-pack panel
  and a real operator-token POST from the browser. Component tests only.
- **NOT_TESTED:** paid model inference. It was explicitly not run.
- **NOT_TESTED:** independent verifier, reviewer and owner acceptance; this
  document is implementer evidence, not certification.
- **KNOWN LIMITATION:** only 15 packs have a recorded content review. The
  remaining 50 canonical packs and three compatibility aliases are
  source-availability-only and block planning until content-reviewed. This is
  intentional fail-closed behavior.
- **KNOWN LIMITATION:** for the 15 reviewed packs, the review scope is the
  named official section headings recorded in the manifest, not a verbatim
  reproduction of the documentation.
- **KNOWN LIMITATION:** the catalog uses `version = "current"` rather than a
  per-release version matrix.
- **KNOWN LIMITATION:** an unreadable or corrupt persisted pack is logged and not
  trusted by the scheduler; it does not invent a replacement beyond the normal
  builtin classification path.
- **KNOWN LIMITATION:** a project scan checks at most 64 projects per pass. The
  durable rotating cursor and explicit skipped count make the partial coverage
  visible; full coverage arrives across successive passes.
- **KNOWN LIMITATION:** the scheduler is an in-process Core task, not a separate
  service. This follows the service-placement default and adds no new process,
  credential boundary or Compose dependency.

No items are marked `BLOCKED` for this WP.

## Integration notes for the shared-file owner

- `native/src/bin/core.rs`: route registration appears in both `api_router` and
  `office_api_router`; startup/shutdown calls and the planning-blocked event are
  registered there.
- `native/src/db/mod.rs`: three event kinds and `record_event_once` were added.
- `native/src/db/planning.rs`: skill-pack reads use `rowid` as the
  insertion-order tiebreaker, source-check/content-review columns are separate,
  and the durable project cursor accessors live there.
- `native/src/db/projects.rs`: project listing is ordered by `created_at,id` so
  the rotating scan is deterministic.
- `native/src/planning/mod.rs`: `SkillReviewBasis` and the separated
  source/content review contract.
- `migrations/0065_skill_refresh_cursor.sql`: durable cursor table.
- `migrations/0066_skill_pack_content_review.sql`: source-check/content-review
  column split.
- The migration owner should keep these numbers when merging the wave.

## Authority and security

Core remains the only authority. The scheduler and catalog are advisory
maintenance and read-only context. The refresh pass cannot approve a plan,
change a lane, close a finding, accept a candidate or grant tools. Events carry
technology names, hashes and review dates, not prompts, credentials or source
bodies.
