# WP3 — skill-pack refresh scheduler and broader technology catalog

**Status:** implementation evidence only — **not** acceptance, certification or a
completion claim. No paid model inference was run. The stale/missing skill gate
was not weakened.

- Branch: `factory/quality-wp3-skill-refresh`
- Implementation commit: `cbf45fd960fe6468feab96461cc99fbded777527`
- Baseline: `bcb8b5b`
- Design authority:
  [ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md)
- Delivery plan:
  [quality-first-delivery-plan.md](quality-first-delivery-plan.md) (WP3)

## Review-finding repairs

Independent review at `c91ea78` found five valid issues. The repair commit
addresses them as follows:

1. **Python/pip blocking:** added a cited `pip` pack
   (`pip.pypa.io` and `packaging.python.org`) and Python-manifest coverage tests
   that prove Django/Flask/Pytest/Ruff projects no longer fail with `Missing`.
2. **Unsupported freshness dates:** added
   [skill-pack catalog review evidence](../evidence/skill-pack-catalog-2026-11-09/review.json)
   covering every pack and cited source URL. On 2026-11-09 each URL was checked
   by HTTP GET (redirects followed) and returned HTTP 200. A test fails if a
   pack, source URL or `reviewedAt` date diverges from that manifest. This is
   source availability/identity verification, not a claim of deep content
   re-review; the limitation is recorded below.
3. **Docker aliases used Podman guidance:** `docker` and `docker-compose` now
   cite `docs.docker.com` and use `docker compose config`; a test rejects
   Podman sources or commands for those names.
4. **Hidden project truncation:** the refresh scan now uses a durable rotating
   cursor (`migrations/0065_skill_refresh_cursor.sql`) with at most 64
   non-overlapping projects per pass. Status and reports expose
   `projectTotal`, `projectsChecked`, `projectsSkipped` and `projectOffset`.
   Tests cover a 70-project rotation across two passes.
5. **Missing Office states:** component tests now cover refresh-status loading,
   load failure, refresh-action failure, technology truncation and deferred
   projects.

## What changed

### Deterministic refresh policy

`native/src/planning/skills.rs` classifies each required technology from the
latest persisted revision and the curated builtin catalog:

| State | Meaning | Blocks planning |
|---|---|---|
| `FRESH` | A persisted revision is valid and before `review_after`, or no persisted revision exists and the builtin pack is current. | No |
| `DUE` | A persisted revision is past `review_after` and no newer, different builtin revision exists. | Yes |
| `REFRESHABLE` | A persisted revision is due and the builtin revision has a later `last_verified_at` and a different content hash. | No; a refresh can append an immutable revision. |
| `STALE` | The builtin pack is past `review_after` or invalid and no usable persisted revision exists. | Yes |
| `MISSING` | Neither a persisted revision nor a builtin pack exists. | Yes |

`build_skill_plan` uses the same classification. Unknown technologies,
stale builtins, invalid packs and due packs without a safe builtin remain
fail-closed. Review dates are never extended in place, and no source URL is
invented.

### Core scheduler and API

`native/src/planning/skill_refresh.rs` adds one Core-owned background task:

- runs one pass immediately, then sleeps for the configured interval;
- interval env `FACTORY_SKILL_REFRESH_INTERVAL_SECONDS`, default `86400`
  seconds, minimum `3600`, maximum `604800`; an invalid value fails Core
  startup instead of silently clamping;
- one task only, with `watch`-based shutdown and an idempotent
  `shutdown_scheduler()` awaited before the database closes;
- serializes passes with a process-wide async mutex;
- scans at most 64 registered project roots per pass using a durable rotating
  cursor, caps status output at 256 technologies and caps event detail at 700
  characters;
- reports checked/skipped/total projects in every status and report so a
  bounded pass never implies full coverage;
- refreshes only a due persisted revision when the builtin revision is newer
  and different; it does not pre-seed the whole catalog;
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
three self-named compatibility aliases. New official-source packs include:
`actix-web`, `angular`, `bun`, `bundler`, `composer`, `csharp`, `deno`,
`diesel`, `django`, `dotnet`, `electron`, `flask`, `github-actions`, `gradle`,
`hyper`, `java`, `kotlin`, `kubernetes`, `laravel`, `maven`, `mysql`, `nuget`,
`php`, `pip`, `pnpm`, `postgresql`, `rails`, `redis`, `reqwest`, `rocket`,
`ruby`, `serde`, `svelte`, `swift`, `tauri`, `terraform`, `tonic`, `tracing`,
`vue` and `yarn`. `docker`, `docker-compose` and `podman` resolve to distinct
self-named compatibility packs so a persisted technology name cannot silently
diverge from its planning need. Docker aliases use Docker-specific sources and
commands.

Every pack validates with HTTPS official sources, best practices,
anti-patterns, security notes, commands, `last_verified_at` and `review_after`.
Coverage tests fail if a detected technology lacks a pack or if the reviewed
source manifest drifts from the catalog.

### Office

The existing Core-backed Skills route has a “Technology knowledge packs” panel:

- shows interval, last/next pass, due/refreshable count, blocking count,
  projects scanned/deferred and the last outcome;
- lists only non-fresh packs with state badges, reason and review deadline;
- exposes an explicit “Run refresh pass” action that calls the authenticated
  Core POST;
- adds English and Spanish strings and component tests for loading, load
  failure, action failure, due/blocked states, truncated technologies, deferred
  projects and the refresh action.

The panel does not invent state or offer a decorative action. The agentic skill
registry remains separate and unchanged in authority.

## Changed files

- `native/src/agentic.rs`
- `native/src/bin/core.rs`
- `native/src/db/mod.rs`
- `native/src/db/planning.rs`
- `native/src/db/projects.rs`
- `native/src/planning/mod.rs`
- `native/src/planning/skill_refresh.rs` (new)
- `native/src/planning/skills.rs`
- `native/src/planning/skills/catalog.rs` (new)
- `migrations/0065_skill_refresh_cursor.sql` (new)
- `office/src/coreApi.ts`
- `office/src/i18n/catalog.en.ts`
- `office/src/i18n/catalog.es.ts`
- `office/src/views/Skills.test.tsx`
- `office/src/views/Skills.tsx`
- `office/src/views/knowledge.css`
- `docs/evidence/skill-pack-catalog-2026-11-09/review.json` (new)
- `docs/product/office-parity.json` (CAP-038 only)
- `docs/implementation/quality-first-planning-status.md` (WP3 row)
- `docs/implementation/wp3-skill-refresh-evidence.md` (this file)

## Verification run

From `native/`:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib planning::skills
8 passed; 0 failed

cargo test -p factory-native --lib planning::skill_refresh
6 passed; 0 failed

cargo test -p factory-native --lib
743 passed; 0 failed; 2 ignored

cargo test -p factory-native --test planning_pipeline
1 passed; 0 failed

cargo test -p factory-native --bin factory-core agentic_plan_and_spec_routes_require_operator_credentials
1 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

The two ignored tests are pre-existing Podman-dependent fixtures; no new test is
ignored.

From `office/`:

```text
npm test -- --run
302 passed across 45 files

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

Catalog review evidence: 68 pack entries and 74 unique source URLs were checked
on 2026-11-09; all returned HTTP 200. The manifest records each pack’s reviewed
date and source statuses. No paid inference was used.

Covered tests include FRESH/DUE/REFRESHABLE/STALE/MISSING classification,
immutable refresh revision, idempotent scheduler pass and events, scheduler
shutdown, missing-technology fail-closed behavior, Python/pip planning,
detected-stack catalog coverage, catalog review-manifest integrity, Docker
source correctness, durable project rotation with skipped counts, endpoint
authentication/bounds and the Office loading/error/action/truncation/deferred
states.

## Limitations and NOT_TESTED

- **NOT_TESTED:** a real Compose Core restart with the scheduler active, the
  scheduler interval boundary in a running container, migration `0065` on an
  upgraded Compose volume, and durable event inspection through the Compose
  stack. Only `podman compose config` was run.
- **NOT_TESTED:** real browser acceptance of the Skills technology-pack panel
  and a real operator-token POST from the browser. Component tests only.
- **NOT_TESTED:** paid model inference. It was explicitly not run.
- **NOT_TESTED:** independent verifier, reviewer and owner acceptance; this
  document is implementer evidence, not certification.
- **KNOWN LIMITATION:** freshness evidence verifies that every cited official
  source URL was reachable on 2026-11-09. It does not claim a deep re-read of
  every document; the curated pack text and the 90-day review window remain the
  maintenance trigger.
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
  insertion-order tiebreaker, and the durable project cursor accessors live
  there.
- `native/src/db/projects.rs`: project listing is ordered by `created_at,id` so
  the rotating scan is deterministic.
- `native/src/planning/mod.rs`: `skill_refresh` module registration.
- `migrations/0065_skill_refresh_cursor.sql`: new durable cursor table. The
  migration owner should keep this number when merging the wave.

## Authority and security

Core remains the only authority. The scheduler and catalog are advisory
maintenance and read-only context. The refresh pass cannot approve a plan,
change a lane, close a finding, accept a candidate or grant tools. Events carry
technology names, hashes and review dates, not prompts, credentials or source
bodies.
