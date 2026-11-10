# WP5 provider/stream/structured-output chaos evidence

**Status:** Implementer self-test evidence for the deterministic WP5 provider
reliability slice. **Not** independent acceptance, certification or live
provider evidence. No paid model inference and no external network egress were
used.

- Branch: `factory/quality-wp5-provider-chaos`
- Baseline commit: `bcb8b5b`
- Review-repair candidate: `d9d8fbefa17c0161a14a8843dd1477d686641406`
- Prior review-repair candidate: `2292044`
- Prior implementation candidate: `77d047f`
- Scope: provider/stream/structured-output reliability only
- Office capability impact: `none` (internal runtime reliability; no new
  user-facing control, route or state)
- Provider sandbox and egress broker topology: unchanged

This revision repairs the independent review findings: provider failures can no
longer salvage a candidate, duplicate suppression covers a bounded replay
window rather than only the immediately previous id, Worker-to-Core turn
settlement/release/cleanup requests have production-path HTTP fixture tests, and
the reservation fixture now enforces exclusive terminal transitions.

## What changed

| File | Change |
|---|---|
| `native/src/provider_stream.rs` | Transport-independent `ReconnectWaiter`, `EventStreamConnector`, injectable `WaitClock`, `TerminalDecision`, cursor replay, duplicate suppression and typed reconnect exhaustion. |
| `native/src/provider_reliability.rs` | Bounded nonconsecutive event-id replay window (`MAX_TRACKED_EVENT_IDS = 4096`); per-turn reconnect reset; `candidate_allowed` completed-terminal gate; bounded structured-output decode/repair. |
| `native/src/provider_lifecycle.rs` (new) | Worker-side HTTP lifecycle for Core-owned turns and sessions: settle, release, terminal evidence and unresolved-session cleanup reconciliation with an injected cleanup receipt. |
| `native/src/runtime.rs` | `open_opencode_event_stream_with_cursor` sends the standard `Last-Event-ID` header; `OpenCodeEventConnector`; shared `classify_opencode_terminal_event`. |
| `native/src/bin/worker.rs` | Removed the provider-error candidate salvage path; final candidate creation now uses `candidate_allowed(Completed, written, all_checks_passed)`. Worker lifecycle calls delegate to `provider_lifecycle`; recovery reconciliation uses the tested HTTP loop. Bounded structured-action repair remains in place. |
| `native/src/lib.rs` | Exports `provider_stream` and `provider_lifecycle`. |
| `native/tests/provider_chaos.rs` | Deterministic offline fault matrix, including nonconsecutive duplicate ids, no-candidate-on-reconnect-exhaustion, production HTTP lifecycle payloads and cleanup reconciliation. |

### Interface notes for the integration owner

- New public modules `factory_native::provider_stream` and
  `factory_native::provider_lifecycle`.
- New public `runtime::OpenCodeEventConnector`,
  `runtime::open_opencode_event_stream_with_cursor` and
  `runtime::classify_opencode_terminal_event`.
- Existing `runtime::open_opencode_event_stream` keeps its signature and
  delegates with no cursor.
- `SseCursor` gained `reset_reconnects` and a bounded seen-id window;
  `StructuredOutputRepair` gained `reset`/`max_repairs`; new
  `TerminalCompletion`/`candidate_allowed`.
- No Core schema, migration, Compose, provider sandbox or egress policy change.

## Review-finding disposition

| Finding | Disposition |
|---|---|
| P1: provider failure could still produce a candidate after frozen checks passed | **Fixed.** `finalize_written_provider_work` and every error-branch salvage call were removed. Reconnect exhaustion, submit failure, missing assistant message and failed/cancelled terminals now return an error without creating a candidate. The normal final path calls `candidate_allowed(Completed, written, all_checks_passed)`. |
| P2: duplicate suppression only covered consecutive ids | **Fixed.** `SseCursor` now keeps a bounded `VecDeque` replay window of 4,096 unique ids. `e1, e2, e1` applies `e1` once. |
| P2: reservation and cleanup tests did not exercise production lifecycle | **Fixed for HTTP lifecycle and transition exclusivity.** `native/src/provider_lifecycle.rs` builds the exact Worker-to-Core requests and is covered by local Core fixtures for settle, release, terminal and cleanup reconciliation. The fixture now accepts separate settle/release reservation ids and rejects a second terminal transition for the same id. The container/Podman external cleanup and ProviderSession state machine remain separately scoped; the matrix below no longer claims they prove live orphan reconciliation. |

## Fault-injection matrix

All fixtures are local and offline. The loopback fixtures bind
`127.0.0.1:0`; no test performs DNS resolution or contacts an external host.
The production-connector test was re-run in this environment after the
reviewer's environment denied binding its loopback listener; all ten repeat runs
below passed.

| # | Fault / behavior | Fixture action | Test | Result |
|---|---|---|---|---|
| 1 | SSE cut mid-turn then reconnect | Connection 1 emits ids `e1`, `e2`, then a transport cut. Reconnect must carry `Last-Event-ID: e2`, replay `e2`, then deliver terminal `e3`. | `sse_cut_reconnects_with_last_event_id_and_does_not_repeat_effects` | PASS — terminal `Idle`; 2 effects, not 3; one candidate; cursors `[None, Some("e2")]`. |
| 2 | Repeated cuts exhaust the reconnect bound | Four connections all cut after `e1`; policy allows three reconnects. | `repeated_cuts_exhaust_the_reconnect_bound_and_fail_closed` | PASS — `ReconnectExhausted { attempts: 3 }`; four connections; zero candidates. |
| 3 | Long turn remains live | 301 provider-observed seconds of heartbeat events followed by terminal, advanced through the injected virtual clock; no real sleep. | `long_turn_stays_live_beyond_the_former_three_hundred_second_deadline` | PASS — virtual elapsed > 300 s; terminal `Idle`; 301 effects; one candidate. |
| 4 | Malformed/truncated structured JSON then a valid repaired object | First output is truncated JSON; the next output is a valid `final` action. | `malformed_structured_json_is_repaired_then_accepted` | PASS — repair 1/2, then accepted; repair budget resets to zero. |
| 5 | Structured repair exhaustion | Three malformed outputs with a two-repair bound. | `structured_json_repair_exhaustion_is_a_typed_failure` | PASS — repair 1, repair 2, then typed `Exhausted { attempts: 2 }`; no free-text fallback. |
| 6 | Consecutive duplicate event ids | Same id `e1` delivered twice, then terminal `e2`. | `duplicate_event_ids_do_not_repeat_effects` | PASS — one effect, one candidate. |
| 7 | Nonconsecutive duplicate event ids | `e1`, `e2`, `e1`, then terminal `e3`. | `nonconsecutive_duplicate_event_ids_are_not_reapplied` | PASS — two effects, not three; one candidate. |
| 8 | Stream closes before a terminal event | Two connections emit a non-terminal event and end; one reconnect is allowed. | `missing_terminal_event_returns_no_candidate` | PASS — `ReconnectExhausted { attempts: 1 }`; zero candidates; replay cursor retained. |
| 9 | Reconnect exhaustion after writes and checks pass | Two cut connections with one reconnect allowed; gate is evaluated with `wrote = true`, `all_checks_passed = true`. | `reconnect_exhaustion_after_checks_pass_yields_no_candidate` | PASS — no terminal; `candidate_allowed(Missing, true, true) == false`; zero candidates. |
| 10 | Provider error terminal | Non-terminal event followed by `session.error`. | `provider_error_terminal_is_not_a_completed_candidate` | PASS — `Failed` terminal, zero completed candidates. |
| 11 | Reservation settlement/release production HTTP | Worker lifecycle client settles one reservation and releases a different reservation against a local Core fixture that enforces exclusive terminal states. | `production_worker_http_lifecycle_settles_releases_and_reconciles_cleanup` | PASS — exact reservation/session/generation/usage payloads observed; settle 200, release 204. |
| 11b | Settle-then-release of the same reservation | Fixture accepts settle for `reservation-both`, then the Worker attempts release of the same id. | `settle_then_release_of_the_same_reservation_is_rejected` | PASS — release receives HTTP 409; typed `Refused`; reservation remains `SETTLED`; no accepted release effect. |
| 11c | Reservation policy exactly-once | A reserved turn is released once; a separate turn is settled once; terminal transitions reject a second effect. | `failed_turn_releases_or_settles_a_reservation_exactly_once` | PASS — settle XOR release exactly once. |
| 12 | Cleanup reconciliation production HTTP | Worker recovery loop lists one unresolved session, obtains a receipt, and posts cleanup to a local Core fixture. | `production_worker_http_lifecycle_settles_releases_and_reconciles_cleanup` | PASS — GET unresolved then POST cleanup with session id, generation 7 and `container_removed_verified_absent`. |
| 13 | Failed cleanup receipt | Receipt provider returns an error. | `failed_cleanup_receipt_never_claims_an_orphan_is_cleaned` | PASS — typed failure; only the unresolved GET occurred; no cleanup claim was posted. |
| 14 | Provider session state machine | Session cut before terminal, reconnect with same identity, terminal, cleanup receipt. | `provider_session_reconnect_and_cleanup_are_exactly_once` | PASS — wrong identity rejected; identical terminal replay idempotent; second terminal rejected after cleanup state; cleanup receipt required and single-use. **Scope:** state-machine behavior only, not live container cleanup. |
| 15 | Production connector forwards the replay cursor | Loopback HTTP fixture checks the actual `Last-Event-ID` request header on reconnect. | `production_connector_forwards_last_event_id_to_a_local_fixture` | PASS — cursors `[None, Some("e2")]`; replayed effect not repeated; terminal reached. |

## Exact commands and results

From the repository root, at review-repair candidate `d9d8fbe`:

```text
cargo fmt --all -- --check
fmt-check: clean

cargo test -p factory-native --test provider_chaos
running 16 tests
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

# Ten consecutive runs of the new suite:
for run in $(seq 1 10); do cargo test -p factory-native --test provider_chaos --quiet; done
run 1 ... run 10: 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
10/10 runs passed; no retry was used to turn a failure green.

cargo test -p factory-native --lib
test result: ok. 738 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 127.22s

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s)
# no warnings
```

The loopback production-connector and Core-fixture tests bound ephemeral
`127.0.0.1` listeners in this environment and passed in every repeat run.

## NOT_TESTED / BLOCKED

- **BLOCKED:** live paid provider inference and every paid provider route. This
  task did not authorize a provider call; no credential, model endpoint or
  upstream request was used.
- **NOT_TESTED:** live long-turn/SSE-cut UAT against OpenCode or Codex inside
  the real provider sandbox. The 301-second long turn above is a deterministic
  virtual-clock fixture, not wall-clock or live-provider evidence.
- **NOT_TESTED:** real provider-specific SSE differences, provider-side
  reconnect behavior, and provider-managed retries.
- **NOT_TESTED:** live Core/Worker/provider process-kill recovery. The new
  cleanup test exercises the Worker-to-Core HTTP reconciliation loop with an
  injected receipt, not real container removal, Podman state or provider
  session deletion.
- **NOT_TESTED:** DNS, TLS, proxy, half-open TCP and external network chaos.
  The only sockets used by the new suite are loopback fixtures.
- **NOT TESTED / NOT CLAIMED:** independent acceptance, Quality Gate, Product
  Owner approval, certification, deployment, or `TRUSTED_LOCAL` isolation.

## Remaining integration work

1. Integration owner review of the new public `provider_stream` and
   `provider_lifecycle` surfaces and the Worker call-site changes.
2. Authorized live long-turn/SSE-cut UAT with a fresh inference envelope,
   including a real reconnect against the installed provider version.
3. Provider-specific connector coverage if Codex or another route exposes a
   different resumable stream protocol; only OpenCode `/event` is wired here.
4. Live orphan-session reconciliation evidence against real provider containers
   and Core state, beyond the injected-receipt HTTP loop.
5. Update [quality-first-planning-status.md](quality-first-planning-status.md)
   when the integration owner accepts this slice; this worktree does not claim
   WP5 complete.

## Security and authority

- No new credential, token, secret, prompt or raw provider payload is logged or
  persisted by the new code.
- The replay cursor is bounded (non-empty, at most 512 bytes, no NUL) before it
  is sent as `Last-Event-ID`.
- The seen-id replay window is bounded to 4,096 unique ids.
- Fixture servers are loopback-only and the tests run offline. The provider
  sandbox and egress broker were not modified.
- A terminal event remains evidence, not authority: Core still owns
  reservations, candidates, budgets and acceptance.
