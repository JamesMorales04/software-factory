# Gym Manager live E2E — 2026-11-09

## Verdict

The final clean run accepted an exact candidate on its first attempt, with no
operator repair and no autonomous repair cycle:

| Fact | Value |
| --- | --- |
| Project | `Gym Manager Clean` — `39e0b83e-9831-4623-b86f-45d6b65d25b2` |
| Project repository | `/projects/gym-manager-clean-1794243215` |
| Request | `b02a100d-dfc4-4563-aa07-828ecab8d902` |
| Campaign | `17c3a6b0-4407-4732-8ce7-143099d58195` |
| Candidate | `d3064bd9-1068-4739-9c61-d1c086db8f33` |
| Candidate commit | `1632877e2f373c3d12372ad6af93a9608363b98d` |
| Gate | `ACCEPTED` |
| Verifier | `PASS`, `DETERMINISTIC`, `EXECUTED` |
| Reviewer | `PASS`, `MODEL`; one non-blocking warning |
| Attempts / candidates | 1 / 1 |
| Repair cycles | 0 |

The request was submitted from Home with the high-level objective:

> Quiero un gestor de gimnasios completo y usable. Decide tú el alcance, la
> arquitectura y las funcionalidades principales. Debe guardar la información y
> estar probado.

This is the clean run after discarding the earlier partial attempts. The earlier
attempts are retained below only as failure evidence.

## What the agent decided

The Lead chose a bounded, std-only Rust CLI:

- `src/model.rs` domain types for members, plans, attendance and payments.
- `src/json.rs` JSON serialization/parsing.
- `src/store.rs` file-backed persistence.
- `src/commands.rs` command handlers.
- `src/cli.rs` / `src/main.rs` CLI parsing and entry point.
- `tests/functional.rs` end-to-end CLI workflows, including unknown flags for
  every subcommand.
- No external crates, no web server, no database, no auth or multi-tenancy.

Assessment: technically sound for the scaffold and budget. The architecture is
cohesive, the persistence is explicit, validation is centralized, and the
functional tests exercise real command paths. The scope is narrower than a
production multi-user gym product: it is a single-user local CLI, not an HTTP
service or web app. That is a legitimate bounded slice, but "complete" should be
read as complete for this CLI scope.

The reviewer's only remaining finding is a warning: `run_with_data` saves after
read-only commands (`member list`, `plan list`, `attendance list`,
`payment list`, `report`), so a read-only command can create/rewrite the data
file or fail on a non-writable path. This is tracked as
[JamesMorales04/gym-manager-clean#1](https://github.com/JamesMorales04/gym-manager-clean/issues/1).

## Defects found and fixed during the test

1. **Provider bridge cut long-lived SSE at 150 s.**
   `native/src/bin/provider_bridge.rs` and `egress_bridge.rs` wrapped
   `copy_bidirectional` in a total 150 s timeout. The OpenCode event stream
   outlived it, so the Worker saw `OpenCode SSE read failed` and finalized
   partial work. Both bridges now leave lifecycle to the provider sandbox and
   broker.
2. **The implementer could finish after writing one file.**
   The Worker now refuses a final response until every frozen check has passed
   in the workspace, and refuses to create a candidate when a provider turn
   ends after writes but before all checks pass.
3. **The verifier sent empty findings.**
   The verifier now emits a blocking `ReviewFinding` with the failed check
   diagnostic. Core includes verifier findings in the repair objective, so a
   repair receives the actual check output.
4. **Build artifacts poisoned the workspace transfer.**
   `cargo build` created `target/` inside the execution volume; the return tar
   exceeded the archive bound and subsequent checks failed with HTTP 413.
   Cargo now writes to an ephemeral executable `/cargo-target` tmpfs, and the
   archive transport bound accepts ignored generated content without counting
   it as source.
5. **`/tmp` was `noexec`, so test binaries could not run.**
   `cargo build` succeeded but `cargo test`/`cargo run` exited 101 because test
   binaries were on a `noexec` tmpfs. The dedicated `/cargo-target` tmpfs is
   executable, nodev and nosuid.
6. **The implementer only checked the literal negative example.**
   Check descriptions are now part of the provider prompt and are treated as
   general acceptance criteria. The first clean accepted candidate includes
   `unknown_flags_are_rejected_for_every_subcommand`.
7. **Campaign pulse showed `Lead Running` after acceptance.**
   `CampaignPulse` now projects the Lead status from the Gate/Campaign state
   (ACCEPTED -> COMPLETED, REPAIR_REQUIRED -> BLOCKED) instead of rendering the
   raw Lead request state.
8. **Issue-intake policy setup could never succeed from the UI.**
   The Office form sends `author_ids: []`, but Core rejected empty author IDs
   and eligibility required an exact ID. Empty now means "any author with the
   acknowledged association", which matches the form. This unblocked the
   `factory:r0` issue on the kanban.

## Application validation

The accepted commit was exported and exercised outside the Factory:

```text
cargo test
  6 functional tests passed
  1 smoke test passed
```

Manual CLI workflow with a fresh JSON file:

```text
plan add --name Monthly --price 49.90 --duration 30
member add --name "Ada Lovelace" --email ada@example.com --plan 1
member list
attendance check-in --member 1 --date 2026-11-09
payment add --member 1 --amount 49.90 --date 2026-11-09
report
```

Observed output:

```text
Added plan #1: Monthly ($49.90, 30 days)
Added member #1: Ada Lovelace
#1 Ada Lovelace <ada@example.com> plan=#1 active=true
Checked in member #1 on 2026-11-09 (attendance #1)
Recorded payment #1: $49.90 from member #1 on 2026-11-09
Members: 1 (active 1)
Plans: 1
Attendance records: 1
Payments: 1 ($49.90)
```

Persistence was verified by reading the JSON after separate process
invocations. Unknown flags for `member`, `plan`, `attendance`, `payment` and
`report` all exit 1 with `error: unknown flag`.

## UI validation

- Office tests: `296 passed` across 45 files.
- `npm run typecheck`: pass.
- `npm run lint`: pass (`biome check`, 161 files).
- Browser audit at 1440×1200 and 390×844 on the accepted request, campaign,
  issues board and projects:
  - exactly one `h1` per page;
  - no horizontal overflow (`scrollWidth == clientWidth`);
  - zero visible controls without an accessible name;
  - zero sub-30px interactive controls.
- Campaign pulse now shows `Lead / Coordinating tasks / Completed` after Gate
  acceptance.
- The issues board shows issue #1 in `READY` at both widths.
- Screenshots are listed below.

## Issue and kanban

The accepted commit was pushed to
`JamesMorales04/gym-manager-clean` and imported through the Projects GitHub tab.
Issue #1 was opened for the reviewer warning and labeled `factory:r0`. After
enabling the bounded intake policy from the Issues UI, the issue appears in the
`READY` column:

- GitHub issue: https://github.com/JamesMorales04/gym-manager-clean/issues/1
- Factory project: `19de62a6-e004-4518-9823-ee2b5c2b6acc`
- Board: `/#/issues`, project `gym-manager-clean`, `Ready: 1`.

The issue was opened on GitHub; Office does not have a "create issue" action.
Office projects GitHub issues into the board and starts governed requests from
them. The Factory reviewer generated the finding that the issue tracks.

## Screenshots

Key screenshots in this directory:

- `clean-objective-1440.png` — high-level objective before submission.
- `clean-run3-start-1440.png` — the clean request at submission.
- `clean-run3-terminal-1440.png` — the clean request at completion.
- `accepted-request-final-1440.png` — completed request, Gate ACCEPTED.
- `accepted-campaign-final-1440.png` — campaign with Lead Completed.
- `accepted-request-390.png` — accepted request at mobile width.
- `issues-kanban-ready-1440.png` — issue #1 in READY on the desktop board.
- `issues-kanban-ready-390.png` — the same board at mobile width.
- `github-project-imported-1440.png` — GitHub-bound project imported.
- `github-project-detail-1440.png` — bound repository and sync state.
- `issues-policy-saved2-1440.png` — intake policy enabled, issue READY.
- `profile-autonomy-1440.png` — autonomy safety-net configuration (not used by
  the accepted run).
- `request-terminal-1440.png`, `repair-terminal-1440.png`,
  `clean-run2-terminal-1440.png` — earlier blocked partial attempts, retained
  as failure evidence.

## Remaining gaps and honest limits

- The accepted candidate has one non-blocking reviewer warning (read-only
  commands persist); issue #1 tracks it. It is not a Gate failure.
- The GitHub issue board was validated with an imported copy of the accepted
  commit. Office has no UI action to bind an already-local project to GitHub;
  that is a UX gap separate from this test.
- `RUSTSEC-2026-0459` remains the known CI merge blocker until sqlx permits
  `libsqlite3-sys >= 0.39.0`; `cargo audit` is not suppressed.
- Two `provider_sandbox` integration tests require a real OpenCode executable
  and bwrap and failed in this host environment. They are not evidence of the
  accepted app path.
- `TRUSTED_LOCAL` is not isolation, certification or independent human
  acceptance. The final acceptance is deterministic Factory evidence, not a
  human sign-off.
