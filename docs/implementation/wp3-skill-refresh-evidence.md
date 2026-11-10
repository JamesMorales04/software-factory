# WP3 — skill-pack refresh scheduler and broader technology catalog

**Status:** implementation evidence only — **not** acceptance, certification or a
completion claim. No paid model inference was run. The stale/missing skill gate
was not weakened.

- Branch: `factory/quality-wp3-skill-refresh`
- Implementation commit: `56fd7dcb8c2cf62e703c8ecf9719de20b7b31352`
- Baseline: `bcb8b5b`
- Design authority:
  [ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md)
- Delivery plan:
  [quality-first-delivery-plan.md](quality-first-delivery-plan.md) (WP3)

## What changed

### Deterministic refresh policy

`native/src/planning/skills.rs` now classifies each required technology from the
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
- scans at most 64 registered project roots, caps status output at 256
  technologies and caps event detail at 700 characters;
- refreshes only a due persisted revision when the builtin revision is newer
  and different; it does not pre-seed the whole catalog;
- records durable idempotent events through `Database::record_event_once`
  (`skill.pack.refresh_due`, `skill.pack.refreshed`,
  `skill.pack.refresh_blocked`);
- logs one bounded summary per pass, not one line per pack.

Operator-authenticated endpoints:

- `GET /agentic/skills/refresh` — current classification, due/blocked counts,
  interval, last/next run, last outcome and bounded status list.
- `POST /agentic/skills/refresh` — explicit deterministic refresh pass.

A blocked `FORMAL_SPEC` plan now also records one idempotent
`skill.pack.refresh_blocked` event on the request while preserving the existing
`SKILL_PACK_REQUIRED` response. The scheduler never approves a plan, hides a
stale pack, lowers a lane or grants authority.

### Broader stack detection and catalog

Stack detection in `native/src/agentic.rs` now recognizes Java/Gradle/Maven,
Kotlin, C#/.NET/NuGet, Ruby/Bundler/Rails, PHP/Composer/Laravel, Swift,
Vue/Svelte/Angular, Django/Flask, PostgreSQL/MySQL/Redis,
Kubernetes/Terraform/GitHub Actions, pnpm/yarn/bun/Deno, Tauri/Electron and the
Rust `actix-web`/`rocket`/`serde`/`tracing`/`reqwest`/`hyper`/`tonic` libraries.
Detection remains bounded to root manifests and at most 512 root entries; it
does not recurse.

The curated catalog moved to
`native/src/planning/skills/catalog.rs` and now has 64 canonical entries. New
official-source packs include: `actix-web`, `angular`, `bun`, `bundler`,
`composer`, `csharp`, `deno`, `diesel`, `django`, `dotnet`, `electron`,
`flask`, `github-actions`, `gradle`, `hyper`, `java`, `kotlin`, `kubernetes`,
`laravel`, `maven`, `mysql`, `nuget`, `php`, `pnpm`, `postgresql`, `rails`,
`redis`, `reqwest`, `rocket`, `ruby`, `serde`, `svelte`, `swift`, `tauri`,
`terraform`, `tonic`, `tracing`, `vue` and `yarn`. `docker`, `docker-compose`
and `podman` resolve to distinct self-named compatibility packs so a persisted
technology name cannot silently diverge from its planning need.

Every pack validates with HTTPS official sources, best practices,
anti-patterns, security notes, commands, `last_verified_at` and `review_after`.
Coverage tests fail if a detected technology lacks a pack.

### Office

The existing Core-backed Skills route now has a “Technology knowledge packs”
panel:

- shows interval, last/next pass, due/refreshable count, blocking count and the
  last outcome;
- lists only non-fresh packs with state badges, reason and review deadline;
- exposes an explicit “Run refresh pass” action that calls the authenticated
  Core POST;
- adds English and Spanish strings and component tests for due/blocked states
  and the refresh action.

The panel does not invent state or offer a decorative action. The agentic skill
registry remains separate and unchanged in authority.

## Changed files

- `native/src/agentic.rs`
- `native/src/bin/core.rs`
- `native/src/db/mod.rs`
- `native/src/db/planning.rs`
- `native/src/planning/mod.rs`
- `native/src/planning/skill_refresh.rs` (new)
- `native/src/planning/skills.rs`
- `native/src/planning/skills/catalog.rs` (new)
- `office/src/coreApi.ts`
- `office/src/i18n/catalog.en.ts`
- `office/src/i18n/catalog.es.ts`
- `office/src/views/Skills.test.tsx`
- `office/src/views/Skills.tsx`
- `office/src/views/knowledge.css`
- `docs/product/office-parity.json` (CAP-038 only)
- `docs/implementation/quality-first-planning-status.md` (WP3 row)
- `docs/implementation/wp3-skill-refresh-evidence.md` (this file)

## Verification run

From `native/`:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib planning::skills
5 passed; 0 failed

cargo test -p factory-native --lib planning::skill_refresh
4 passed; 0 failed

cargo test -p factory-native --lib
738 passed; 0 failed; 2 ignored

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
npm ci --ignore-scripts
214 packages installed (local dependency install only; not acceptance)

npm test -- --run
298 passed across 45 files

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
immutable refresh revision, idempotent scheduler pass and events, scheduler
shutdown, missing-technology fail-closed behavior, detected-stack catalog
coverage, endpoint authentication/bounds and the Office due/blocked/refresh
states.

## Limitations and NOT_TESTED

- **NOT_TESTED:** a real Compose Core restart with the scheduler active, the
  scheduler interval boundary in a running container, and durable event
  inspection through the Compose stack. Only `podman compose config` was run.
- **NOT_TESTED:** real browser acceptance of the Skills technology-pack panel
  and a real operator-token POST from the browser. Component tests only.
- **NOT_TESTED:** live upstream documentation verification. Builtin sources are
  curated official URLs; the scheduler does not fetch the network. Refreshing a
  builtin catalog revision still requires a reviewed code change, not a web
  fetch.
- **NOT_TESTED:** paid model inference. It was explicitly not run.
- **NOT_TESTED:** independent verifier, reviewer and owner acceptance; this
  document is implementer evidence, not certification.
- **KNOWN LIMITATION:** the catalog uses `version = "current"` rather than a
  per-release version matrix.
- **KNOWN LIMITATION:** an unreadable or corrupt persisted pack is logged and not
  trusted by the scheduler; it does not invent a replacement beyond the normal
  builtin classification path.
- **KNOWN LIMITATION:** the scheduler is an in-process Core task, not a separate
  service. This follows the service-placement default and adds no new process,
  credential boundary or Compose dependency.

No items are marked `BLOCKED` for this WP.

## Integration notes for the shared-file owner

- `native/src/bin/core.rs`: route registration appears in both `api_router` and
  `office_api_router`; startup/shutdown calls and the planning-blocked event are
  registered there.
- `native/src/db/mod.rs`: three event kinds and `record_event_once` were added.
- `native/src/db/planning.rs`: skill-pack reads now use `rowid` as the
  insertion-order tiebreaker for the latest immutable revision.
- `native/src/planning/mod.rs`: `skill_refresh` module registration.
- No migration was added.

## Authority and security

Core remains the only authority. The scheduler and catalog are advisory
maintenance and read-only context. The refresh pass cannot approve a plan,
change a lane, close a finding, accept a candidate or grant tools. Events carry
technology names, hashes and review dates, not prompts, credentials or source
bodies.
