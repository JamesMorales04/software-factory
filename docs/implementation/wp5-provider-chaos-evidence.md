# WP5 provider/stream/structured-output chaos evidence

**Status:** Implementer self-test evidence for the deterministic WP5 provider
reliability slice. **Not** independent acceptance, certification or live
provider evidence. No paid model inference and no external network egress were
used.

- Branch: `factory/quality-wp5-provider-chaos`
- Baseline commit: `bcb8b5b`
- Implementation candidate: `77d047f8a06f18222c5c0c33f753b84536474e78`
- Scope: provider/stream/structured-output reliability only
- Office capability impact: `none` (internal runtime reliability; no new
  user-facing control, route or state)
- Provider sandbox and egress broker topology: unchanged

## What changed

| File | Change |
|---|---|
| `native/src/provider_stream.rs` (new) | Transport-independent `ReconnectWaiter`, `EventStreamConnector`, injectable `WaitClock`, `TerminalDecision`, cursor replay, duplicate-event suppression and typed reconnect exhaustion. |
| `native/src/provider_reliability.rs` | `SseCursor::reset_reconnects` (per-turn bound without losing the replay cursor); `StructuredOutputError`; `decode_structured_output`; repair reset/accessors; deterministic unit tests. |
| `native/src/runtime.rs` | `open_opencode_event_stream_with_cursor` sends the standard `Last-Event-ID` header; `OpenCodeEventConnector`; shared `classify_opencode_terminal_event` used by the existing terminal waiter. |
| `native/src/bin/worker.rs` | Worker now uses `ReconnectWaiter` for OpenCode turn terminals, so reconnects resume from the last complete event id. Malformed structured actions use the bounded repair gate (`MAX_STRUCTURED_REPAIRS = 2`) and then fail typed. No hidden turn deadline. |
| `native/src/lib.rs` | Exports `provider_stream`. |
| `native/tests/provider_chaos.rs` (new) | Deterministic offline fault matrix: in-memory scripted streams plus one loopback-only HTTP fixture. |

### Interface notes for the integration owner

- New public module `factory_native::provider_stream`.
- New public `runtime::OpenCodeEventConnector`,
  `runtime::open_opencode_event_stream_with_cursor` and
  `runtime::classify_opencode_terminal_event`.
- Existing `runtime::open_opencode_event_stream` keeps its signature and
  delegates with no cursor.
- `SseCursor` gained `reset_reconnects`; `StructuredOutputRepair` gained
  `reset`/`max_repairs`.
- No Core schema, migration, Compose, provider sandbox or egress policy change.

## Fault-injection matrix

All fixtures are local and offline. The loopback fixture binds
`127.0.0.1:0`; no test performs DNS resolution or contacts an external host.

| # | Fault / behavior | Fixture action | Test | Result |
|---|---|---|---|---|
| 1 | SSE cut mid-turn then reconnect | Connection 1 emits ids `e1`, `e2`, then a transport cut. Reconnect must carry `Last-Event-ID: e2`, replay `e2`, then deliver terminal `e3`. | `sse_cut_reconnects_with_last_event_id_and_does_not_repeat_effects` | PASS — terminal `Idle`; 2 effects, not 3; one candidate; cursors `[None, Some("e2")]`. |
| 2 | Repeated cuts exhaust the reconnect bound | Four connections all cut after `e1`; policy allows three reconnects. | `repeated_cuts_exhaust_the_reconnect_bound_and_fail_closed` | PASS — `ReconnectExhausted { attempts: 3 }`; four connections; zero candidates. |
| 3 | Long turn remains live | 301 provider-observed seconds of heartbeat events followed by terminal, advanced through the injected virtual clock; no real sleep. | `long_turn_stays_live_beyond_the_former_three_hundred_second_deadline` | PASS — virtual elapsed > 300 s; terminal `Idle`; 301 effects; one candidate. |
| 4 | Malformed/truncated structured JSON then a valid repaired object | First output is truncated JSON; the next output is a valid `final` action. | `malformed_structured_json_is_repaired_then_accepted` | PASS — repair 1/2, then accepted; repair budget resets to zero. |
| 5 | Structured repair exhaustion | Three malformed outputs with a two-repair bound. | `structured_json_repair_exhaustion_is_a_typed_failure` | PASS — repair 1, repair 2, then typed `Exhausted { attempts: 2 }`; no free-text fallback. |
| 6 | Duplicate event ids | Same id `e1` is delivered twice on one stream, followed by terminal `e2`. | `duplicate_event_ids_do_not_repeat_effects` | PASS — one effect, one candidate. |
| 7 | Stream closes before a terminal event | Two connections emit a non-terminal event and end; one reconnect is allowed. | `missing_terminal_event_returns_no_candidate` | PASS — `ReconnectExhausted { attempts: 1 }`; zero candidates; replay cursor retained. |
| 8 | Provider error terminal | Non-terminal event followed by `session.error`. | `provider_error_terminal_is_not_a_completed_candidate` | PASS — `Failed` terminal, zero completed candidates. |
| 9 | Reservation cleanup on failure | A reserved turn is released once; a separate turn is settled once; terminal transitions reject a second effect. | `failed_turn_releases_or_settles_a_reservation_exactly_once` | PASS — release XOR settle exactly once. |
| 10 | Provider session reconnect/cleanup | Session cut before terminal, reconnect with same identity, terminal, cleanup receipt. | `provider_session_reconnect_and_cleanup_are_exactly_once` | PASS — wrong identity rejected; identical terminal replay idempotent; second terminal rejected after cleanup state; cleanup receipt required and single-use. |
| 11 | Production connector forwards the replay cursor | Loopback HTTP fixture checks the actual `Last-Event-ID` request header on reconnect. | `production_connector_forwards_last_event_id_to_a_local_fixture` | PASS — cursors `[None, Some("e2")]`; replayed effect not repeated; terminal reached. |
| 12 | Reconnect/repair policy unit coverage | Library tests for backoff bounds, cursor dedupe, reconnect reset, reservation exactly-once, repair acceptance/exhaustion. | `provider_reliability` and `provider_stream` unit tests | PASS. |

## Exact commands and results

From the repository root, at candidate `77d047f`:

```text
cargo fmt --all -- --check
fmt-check: clean

cargo test -p factory-native --test provider_chaos
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

# Ten consecutive runs of the new suite:
for run in $(seq 1 10); do cargo test -p factory-native --test provider_chaos --quiet; done
run 1  ... run 10: 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
10/10 runs passed; no retry was used to turn a failure green.

cargo test -p factory-native --lib
test result: ok. 736 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 118.70s

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s)
# no warnings
```

Targeted provider library tests also passed:

```text
cargo test -p factory-native --lib provider_
test result: ok. 103 passed; 0 failed; 0 ignored; 0 measured; 635 filtered out
```

## NOT_TESTED / BLOCKED

- **BLOCKED:** live paid provider inference and every paid provider route. This
  task did not authorize a provider call; no credential, model endpoint or
  upstream request was used.
- **NOT_TESTED:** live long-turn/SSE-cut UAT against OpenCode or Codex inside
  the real provider sandbox. The 301-second long turn above is a deterministic
  virtual-clock fixture, not wall-clock or live-provider evidence.
- **NOT_TESTED:** real provider-specific SSE differences, provider-side
  reconnect behavior, and provider-managed retries.
- **NOT_TESTED:** live Core/Worker/provider process-kill recovery and real
  orphan-session reconciliation. The deterministic session state machine and
  existing Core DB tests are the only evidence here.
- **NOT_TESTED:** DNS, TLS, proxy, half-open TCP and external network chaos.
  The only socket used by the new suite is a loopback fixture.
- **NOT TESTED / NOT CLAIMED:** independent acceptance, Quality Gate, Product
  Owner approval, certification, deployment, or `TRUSTED_LOCAL` isolation.

## Remaining integration work

1. Integration owner review of the new public `provider_stream` surface and the
   Worker call-site refactor in `run_provider_tool_loop`/`run_provider_patch`.
2. Authorized live long-turn/SSE-cut UAT with a fresh inference envelope,
   including a real reconnect against the installed provider version.
3. Provider-specific connector coverage if Codex or another route exposes a
   different resumable stream protocol; only OpenCode `/event` is wired here.
4. Product decision on the existing "all frozen checks passed after a later
   turn error" salvage path. The deterministic test proves a missing terminal
   with no completed checks yields no candidate; that pre-existing recovery
   path was intentionally left unchanged to avoid an unauthorized behavior
   change.
5. Update [quality-first-planning-status.md](quality-first-planning-status.md)
   when the integration owner accepts this slice; this worktree does not claim
   WP5 complete.

## Security and authority

- No new credential, token, secret, prompt or raw provider payload is logged or
  persisted by the new code.
- The replay cursor is bounded (non-empty, at most 512 bytes, no NUL) before it
  is sent as `Last-Event-ID`.
- The fixture server is loopback-only and the tests run offline. The provider
  sandbox and egress broker were not modified.
- A terminal event remains evidence, not authority: Core still owns
  reservations, candidates, budgets and acceptance.
