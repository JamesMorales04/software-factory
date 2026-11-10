# WP12 deterministic chaos and cross-workstream matrix

**Status:** implementer evidence for the Quality-first Wave 1/2 slices.
**Not** independent acceptance, certification, deployment or a live-provider
claim. The matrix maps every deterministic fault scenario to the test that owns
it and to the command that runs it. Live, paid, browser-only and real-Podman
paths that the deterministic suite cannot prove are recorded as `NOT_TESTED`
instead of being counted as passes.

- Branch: `factory/quality-wave12-chaos-matrix`
- Baseline: `452bb40` (`factory/quality-wave12-integration`)
- Candidate: `4769b9ed0ca1b67c2afde2edafdcfb02dca14c1c` (tree
  `9d094cc9188f27270e470fb608ebf4980ddbac1f`, clean)
- Acceptance script: [`scripts/quality-wave12-acceptance.sh`](../../scripts/quality-wave12-acceptance.sh)
- Candidate-bound evidence: [`docs/evidence/quality-wave12-chaos-2026-11-10/manifest.json`](../evidence/quality-wave12-chaos-2026-11-10/manifest.json)
- Related evidence: [WP5 provider chaos](wp5-provider-chaos-evidence.md),
  [WP6 workspace recovery](wp6-workspace-recovery-evidence.md),
  [WP7 OpenSpec Compose](wp7-openspec-compose-evidence.md),
  [WP9 reviewer independence](wp9-reviewer-independence-evidence.md),
  [WP11 trace view](wp11-trace-view-evidence.md)

## How to reproduce

```sh
scripts/quality-wave12-acceptance.sh --require-clean
```

The script runs formatting, the complete library and binary suites, the targeted
integration suites, the focused workflow/quality/trace/reviewer/Gate filters,
three consecutive `provider_chaos` runs, Clippy with `-D warnings`, the Office
`test`/`typecheck`/`lint` commands, `podman compose config` for the base and
`openspec-validation` profiles, and the Compose policy checker plus negative
tests for both profiles. It is read-only with respect to the installed stack:
it never runs `podman compose up`, builds no image and starts no provider.

Only a full run can print `RESULT: PASS`. `--skip-office` or `--skip-compose`
is diagnostic only and produces `RESULT: PARTIAL` with exit 3. A failed step
prints `RESULT: FAIL` and exits 1. `--no-install` fails closed when Office
dependencies are missing or stale. `scripts/quality-wave12-acceptance.test.sh`
exercises the skip/no-install control flow without running the full suite.

The JSON policy input is rendered by `podman compose ... config` into
`target/quality-wave12/compose-base.yaml` and
`target/quality-wave12/compose-openspec.yaml`, then normalized from those saved
files by `docker-compose -f <saved-file> config --format json`; this host's
`podman-compose` does not implement `--format json`. The normalizer is a parser
only, not a runtime dependency of the product. Test temporary files use
`/var/tmp/factory-wave12`, outside the repository, because some tests assert
that the temp root is not a Git worktree.

## Candidate result

The recorded run used candidate `4769b9e`, a clean worktree and
`--require-clean`:

| Check | Exact result |
|---|---|
| Acceptance script | 19/19 steps PASS, 0 FAIL, 0 skipped steps; 225 s |
| Library suite | 830 passed, 0 failed, 4 ignored |
| Binary suites | 125 passed, 0 failed (Core 38, runtime broker 29, worker 17, supervisor 9, verifier 7, architecture 8, repo hygiene 7, reviewer 4, dependency acquire 3, provider smoke 3) |
| Targeted integration | 29 passed, 0 failed (planning 1, provider chaos 16, dependency HTTP 6, OpenSpec 5, migration 1) |
| Focused filters | workflow 16, quality 12, trace 17, review_roles 9, gate_ 18 — all passed |
| Provider chaos repeats | 3/3 runs, 16 passed each, 0 failed |
| Clippy | clean with `-D warnings` |
| Office | 46 files / 317 tests passed; typecheck clean; lint clean (163 files) |
| Compose config | base 14 services; `openspec-validation` 15 services; both exit 0 |
| Compose policy | PASS both profiles; 36 negative tests, 8 skipped base / 5 skipped profile |

The other flaky-sensitive suites and the focused filters were repeated three
times each after the script; all runs passed (see manifest `repeatRuns`). No
retry was used to turn a failure green. The full captured output is
[acceptance.log](../evidence/quality-wave12-chaos-2026-11-10/acceptance.log)
and
[repeat-runs.log](../evidence/quality-wave12-chaos-2026-11-10/repeat-runs.log).

## Matrix

Commands are run from the repository root. `LIB` means
`cargo test -p factory-native --locked --lib`; `IT` means
`cargo test -p factory-native --locked --test <suite>`; `BIN` means
`cargo test -p factory-native --locked --bin <binary>`. Where a row lists
multiple tests, the command cell lists a filter for each listed test; filters
may select additional tests in the same area.

### Provider, stream and structured output (WP5)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| P1 | SSE cut mid-turn, reconnect with `Last-Event-ID`, replay without duplicate effects | `sse_cut_reconnects_with_last_event_id_and_does_not_repeat_effects`; `production_connector_forwards_last_event_id_to_a_local_fixture` | `IT provider_chaos sse_cut_reconnects_with_last_event_id_and_does_not_repeat_effects`; `IT provider_chaos production_connector_forwards_last_event_id_to_a_local_fixture` | PASS |
| P2 | Repeated cuts exhaust the bounded reconnect budget and fail closed | `repeated_cuts_exhaust_the_reconnect_bound_and_fail_closed`; `reconnect_exhaustion_after_checks_pass_yields_no_candidate` | `IT provider_chaos repeated_cuts_exhaust_the_reconnect_bound_and_fail_closed`; `IT provider_chaos reconnect_exhaustion_after_checks_pass_yields_no_candidate` | PASS |
| P3 | Long turn stays live beyond the former 300-second deadline | `long_turn_stays_live_beyond_the_former_three_hundred_second_deadline` | `IT provider_chaos long_turn_stays_live_beyond_the_former_three_hundred_second_deadline` | PASS |
| P4 | Malformed structured JSON is repaired; repair exhaustion is typed | `malformed_structured_json_is_repaired_then_accepted`; `structured_json_repair_exhaustion_is_a_typed_failure`; `structured_output_repairs_are_bounded` | `IT provider_chaos structured_json`; `LIB structured_output_repairs_are_bounded` | PASS |
| P5 | Consecutive and nonconsecutive duplicate event ids do not repeat effects | `duplicate_event_ids_do_not_repeat_effects`; `nonconsecutive_duplicate_event_ids_are_not_reapplied`; `cursor_deduplicates_nonconsecutive_event_ids_within_a_bounded_window` | `IT provider_chaos duplicate`; `LIB cursor_deduplicates_nonconsecutive_event_ids_within_a_bounded_window` | PASS |
| P6 | Stream closes without a terminal event; provider error is not a completed candidate | `missing_terminal_event_returns_no_candidate`; `provider_error_terminal_is_not_a_completed_candidate`; `missing_terminal_exhausts_reconnect_budget` | `IT provider_chaos terminal`; `LIB missing_terminal_exhausts_reconnect_budget` | PASS |
| P7 | Reservation settle/release is exactly once; settle-then-release is rejected | `failed_turn_releases_or_settles_a_reservation_exactly_once`; `settle_then_release_of_the_same_reservation_is_rejected`; `production_worker_http_lifecycle_settles_releases_and_reconciles_cleanup` | `IT provider_chaos reservation`; `IT provider_chaos lifecycle` | PASS |
| P8 | Orphan provider session and cleanup receipt are reconciled without a false cleanup claim | `provider_session_reconnect_and_cleanup_are_exactly_once`; `failed_cleanup_receipt_never_claims_an_orphan_is_cleaned`; `production_worker_http_lifecycle_settles_releases_and_reconciles_cleanup` | `IT provider_chaos cleanup`; `IT provider_chaos lifecycle` | PASS |

### Durable workflow, idempotency and outbox (WP1)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| W1 | Kill/restart at every forward, repair, replan and terminal state resumes the exact durable state and revision | `restart_at_each_state_preserves_durable_workflow_and_resumes`; `workflow_state_and_effects_survive_reopen` | `LIB restart_at_each_state_preserves_durable_workflow_and_resumes`; `LIB workflow_state_and_effects_survive_reopen` | PASS |
| W2 | Idempotency replay returns the prior result; conflicting replay is rejected | `idempotency_replay_returns_prior_result_and_conflicts_are_rejected`; `ensure_workflow_run_is_idempotent_and_rejects_conflicts` | `LIB idempotency_replay_returns_prior_result_and_conflicts_are_rejected`; `LIB ensure_workflow_run_is_idempotent_and_rejects_conflicts` | PASS |
| W3 | Outbox lifecycle is replay-safe and reconciled entries cannot regress | `outbox_lifecycle_is_replay_safe` | `LIB outbox_lifecycle_is_replay_safe` | PASS |
| W4 | Recovery returns only running steps; reclaim/finish is exactly once | `recovery_returns_only_running_steps_and_finish_is_exactly_once` | `LIB recovery_returns_only_running_steps_and_finish_is_exactly_once` | PASS |

### Workspace, snapshots and cleanup (WP6)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| S1 | Content-addressed snapshot round-trips and excludes Git/generated target artifacts | `snapshots_excludes_git_and_round_trips_regular_files`; `ignores_cargo_target_hardlinks_from_podman_volume_export`; `checkpoint_hash_is_content_addressed_and_replay_safe` | `LIB snapshots_excludes_git_and_round_trips_regular_files`; `LIB ignores_cargo_target_hardlinks_from_podman_volume_export`; `LIB checkpoint_hash_is_content_addressed_and_replay_safe` | PASS |
| S2 | Corrupt/truncated archive fails closed and never replaces the worktree | `corrupt_exported_archive_fails_closed_without_replacing_the_worktree`; `rejects_truncated_archives_and_removes_partial_staging`; `rejects_archive_truncated_mid_entry_and_leaves_existing_destination`; `rejects_symlinks_and_unsafe_archives` | `LIB corrupt_exported_archive_fails_closed_without_replacing_the_worktree`; `LIB rejects_truncated_archives_and_removes_partial_staging`; `LIB rejects_archive_truncated_mid_entry_and_leaves_existing_destination`; `LIB rejects_symlinks_and_unsafe_archives` | PASS |
| S3 | Owner-token spoofing, missing token and wrong owner fail closed | `remove_volume_requires_the_authenticated_owner`; `import_export_execute_and_provider_start_require_the_authenticated_owner`; `owner_tokens_are_distinct_and_authenticate_exactly`; `constant_time_compare_requires_exact_match` | `BIN factory-runtime-broker remove_volume_requires_the_authenticated_owner`; `BIN factory-runtime-broker import_export_execute_and_provider_start_require_the_authenticated_owner`; `BIN factory-runtime-broker owner_tokens_are_distinct_and_authenticate_exactly`; `BIN factory-runtime-broker constant_time_compare_requires_exact_match` | PASS |
| S4 | Wrong-owner delete/reset and foreign pre-existing resources are refused without mutation | `remove_volume_rejects_missing_owner_and_missing_resource`; `remove_container_requires_the_expected_owner_label`; `create_volume_refuses_a_preexisting_foreign_volume`; `concurrent_delete_and_reset_never_removes_the_replacement` | `BIN factory-runtime-broker remove_volume_rejects_missing_owner_and_missing_resource`; `BIN factory-runtime-broker remove_container_requires_the_expected_owner_label`; `BIN factory-runtime-broker create_volume_refuses_a_preexisting_foreign_volume`; `BIN factory-runtime-broker concurrent_delete_and_reset_never_removes_the_replacement` | PASS |
| S5 | Lock races serialize create/delete/reset and do not block unrelated names | `concurrent_create_for_different_owners_serializes_per_name`; `execution_cleanup_serializes_with_container_delete`; `long_execution_does_not_block_an_unrelated_resource` | `BIN factory-runtime-broker concurrent_create_for_different_owners_serializes_per_name`; `BIN factory-runtime-broker execution_cleanup_serializes_with_container_delete`; `BIN factory-runtime-broker long_execution_does_not_block_an_unrelated_resource` | PASS |
| S6 | Lock-table saturation keeps one identity per name and avoids overflow deadlock | `resource_locks_keep_one_identity_per_name_at_saturation`; `execution_cleanup_works_when_volume_and_container_share_overflow`; `read_capped_stops_at_the_limit`; `inspect_output_is_bounded_before_parsing` | `BIN factory-runtime-broker resource_locks_keep_one_identity_per_name_at_saturation`; `BIN factory-runtime-broker execution_cleanup_works_when_volume_and_container_share_overflow`; `BIN factory-runtime-broker read_capped_stops_at_the_limit`; `BIN factory-runtime-broker inspect_output_is_bounded_before_parsing` | PASS |
| S7 | Cancellation keeps the volume blocked until detached cleanup confirms absence; normal completion releases it | `cancelled_execution_keeps_volume_blocked_until_cleanup_is_confirmed`; `normal_execution_releases_the_volume_marker_after_cleanup`; `cancellation_kills_a_running_execution_and_blocks_a_late_retry`; `duplicate_active_container_name_is_rejected_without_removal` | `BIN factory-runtime-broker cancelled_execution_keeps_volume_blocked_until_cleanup_is_confirmed`; `BIN factory-runtime-broker normal_execution_releases_the_volume_marker_after_cleanup`; `LIB cancellation_kills_a_running_execution_and_blocks_a_late_retry`; `BIN factory-runtime-broker duplicate_active_container_name_is_rejected_without_removal` | PASS |

### Dependency supply chain (WP4)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| D1 | Lockfile mismatch fails before commands run and leaves vendor untouched | `lockfile_mismatch_fails_before_running_commands`; `workspace_evidence_requires_lock_toolchain_vendor_and_source_replacement`; `unsafe_paths_registry_bounds_and_lockfile_mismatch_do_not_touch_vendor` | `LIB lockfile_mismatch_fails_before_running_commands`; `LIB workspace_evidence_requires_lock_toolchain_vendor_and_source_replacement`; `IT dependency_broker_http unsafe_paths_registry_bounds_and_lockfile_mismatch_do_not_touch_vendor` | PASS |
| D2 | Command failure preserves the previous vendor tree | `failed_acquisition_preserves_the_previous_vendor_tree`; `command_failure_preserves_the_previous_vendor_tree` | `LIB failed_acquisition_preserves_the_previous_vendor_tree`; `IT dependency_broker_http command_failure_preserves_the_previous_vendor_tree` | PASS |
| D3 | Atomic promotion/rollback and receipt binding | `acquisition_promotes_atomically_and_cleans_staging`; `valid_request_promotes_vendor_and_evidence_atomically`; `receipt_validation_binds_request_image_and_evidence_paths` | `LIB acquisition_promotes_atomically_and_cleans_staging`; `IT dependency_broker_http valid_request_promotes_vendor_and_evidence_atomically`; `LIB receipt_validation_binds_request_image_and_evidence_paths` | PASS |
| D4 | Direct egress is denied; only allowlisted CONNECT tunnels are permitted | `non_allowlisted_connect_is_denied`; `allowlisted_connect_tunnels_bytes`; `pinned_resolution_uses_the_allowlisted_address` | `LIB non_allowlisted_connect_is_denied`; `LIB allowlisted_connect_tunnels_bytes`; `LIB pinned_resolution_uses_the_allowlisted_address` | PASS |
| D5 | Proxy/registry allowlist and internal-network invariants are strict | `dependency_registry_allowlist_is_strict`; `existing_egress_network_must_remain_internal_and_managed`; `allowlist_parsing_is_strict` | `BIN factory-runtime-broker dependency_registry_allowlist_is_strict`; `BIN factory-runtime-broker existing_egress_network_must_remain_internal_and_managed`; `LIB allowlist_parsing_is_strict` | PASS |
| D6 | Image identity/pin is verified before acquisition | `image_identity_and_egress_configuration_are_strict`; `image_allowlist_rejects_flags_and_empty_entries`; `import_images_are_strict` | `LIB image_identity_and_egress_configuration_are_strict`; `BIN factory-runtime-broker image_allowlist_rejects_flags_and_empty_entries`; `BIN factory-runtime-broker import_images_are_strict` | PASS |
| D7 | Real rootless Podman acquisition enforces egress and pins the image | `podman_acquirer_enforces_egress_and_pins_the_image` | `FACTORY_PODMAN_DEPENDENCY_TEST=1 cargo test -p factory-native --locked --test dependency_broker_podman` | NOT_TESTED (opt-in real Podman) |
| D8 | Live registry acquisition is explicit and optional | `live_registry_acquisition_is_explicit_and_optional` | `FACTORY_LIVE_DEPENDENCY_FETCH=1 cargo test -p factory-native --locked --test dependency_acquire_live` | NOT_TESTED (live registry) |

### Reviewer independence and Gate (WP9)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| R1 | Same provider/model with distinct authenticated principals is independent | `same_provider_and_model_with_distinct_principals_is_independent`; `gate_accepts_distinct_principals_with_same_model_and_exact_role_assignments` | `LIB same_provider_and_model_with_distinct_principals_is_independent`; `LIB gate_accepts_distinct_principals_with_same_model_and_exact_role_assignments` | PASS |
| R2 | Duplicate reviewer principal cannot count twice | `duplicate_reviewer_principal_is_rejected`; `gate_rejects_a_second_receipt_from_the_same_principal` | `LIB duplicate_reviewer_principal_is_rejected`; `LIB gate_rejects_a_second_receipt_from_the_same_principal` | PASS |
| R3 | Single identity cannot staff a critical/elevated multi-role plan | `one_reviewer_for_critical_or_elevated_is_rejected`; `identity_assignments_fail_closed_when_a_role_is_missing` | `LIB one_reviewer_for_critical_or_elevated_is_rejected`; `LIB identity_assignments_fail_closed_when_a_role_is_missing` | PASS |
| R4 | Extra-role receipt blocks acceptance even when required roles pass | `gate_blocks_an_extra_role_receipt_even_when_required_roles_pass`; `reviewer_receipt_rejects_a_role_outside_the_frozen_plan` | `LIB gate_blocks_an_extra_role_receipt_even_when_required_roles_pass`; `LIB reviewer_receipt_rejects_a_role_outside_the_frozen_plan` | PASS |
| R5 | Roleless legacy receipt blocks when the frozen plan requires roles | `gate_blocks_a_roleless_legacy_receipt_when_required_roles_exist` | `LIB gate_blocks_a_roleless_legacy_receipt_when_required_roles_exist` | PASS |
| R6 | Missing or unexpected frozen assignment set blocks the Gate | `gate_blocks_required_roles_without_frozen_principal_assignments`; `gate_rejects_an_assignment_set_with_an_unexpected_role` | `LIB gate_blocks_required_roles_without_frozen_principal_assignments`; `LIB gate_rejects_an_assignment_set_with_an_unexpected_role` | PASS |
| R7 | Real Compose Campaign with two or more reviewer receipts aggregated by the Gate | n/a (deterministic DB/plan/Gate only) | `scripts/quality-wave12-acceptance.sh` does not start the stack | NOT_TESTED (live Compose Campaign) |

### OpenSpec (WP7)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| O1 | Deterministic render and pinned-version validation flow | `rendering_is_deterministic_and_has_required_sections`; `pinned_version_is_enforced_and_validation_is_bound_to_the_change`; `rendered_files_drive_the_pinned_validation_runner`; `committed_fixture_matches_the_core_renderer` | `LIB rendering_is_deterministic_and_has_required_sections`; `LIB pinned_version_is_enforced_and_validation_is_bound_to_the_change`; `IT openspec_compose_validation rendered_files_drive_the_pinned_validation_runner`; `IT openspec_compose_validation committed_fixture_matches_the_core_renderer` | PASS (deterministic runner; real pinned-image run is WP7 evidence) |
| O2 | Archive is idempotent and immutable | `archive_contract_is_idempotent_and_rejects_stored_artifact_drift`; `openspec_archive_is_idempotent_and_immutable`; `archive_outcome_requires_a_validated_projection_and_uses_the_archive_path` | `IT openspec_compose_validation archive_contract_is_idempotent_and_rejects_stored_artifact_drift`; `LIB openspec_archive_is_idempotent_and_immutable`; `LIB archive_outcome_requires_a_validated_projection_and_uses_the_archive_path` | PASS |
| O3 | Stored-artifact, projection and stale-revision drift are rejected | `read_model_rejects_stale_or_mismatched_records`; `read_model_rejects_a_changed_artifact`; `openspec_archive_rejects_a_revision_that_is_no_longer_latest`; `stored_artifact_digest_is_recomputed_before_decoding` | `LIB read_model_rejects_stale_or_mismatched_records`; `IT openspec_compose_validation read_model_rejects_a_changed_artifact`; `LIB openspec_archive_rejects_a_revision_that_is_no_longer_latest`; `LIB stored_artifact_digest_is_recomputed_before_decoding` | PASS |
| O4 | Real Core-backed projection and pinned OpenSpec CLI in Compose | n/a (WP7 ran a Core-rendered fixture; no paid model planning projection) | manual WP7 Compose job, not part of this script | NOT_TESTED (real Core-backed projection) |

### Trace, replay and redaction (WP11)

| # | Scenario | Deterministic test(s) | Command | Result |
|---|---|---|---|---|
| T1 | Duplicate sequence events are collapsed/reported | `duplicate_gap_open_step_and_state_divergence_are_reported`; `identical_duplicate_sequences_are_collapsed_and_reported`; `conflicting_duplicate_sequences_are_rejected` | `LIB duplicate_gap_open_step_and_state_divergence_are_reported`; `LIB identical_duplicate_sequences_are_collapsed_and_reported`; `LIB conflicting_duplicate_sequences_are_rejected` | PASS |
| T2 | Missing transitions/gaps are reported without inventing state | `chain_gaps_are_reported_without_inventing_state`; `trace_loader_reports_a_gap_and_bounds_the_newest_events` | `LIB chain_gaps_are_reported_without_inventing_state`; `LIB trace_loader_reports_a_gap_and_bounds_the_newest_events` | PASS |
| T3 | Malformed transition/event rows are flagged | `malformed_event_is_reported`; `malformed_transition_rows_are_flagged` | `LIB malformed_event_is_reported`; `LIB malformed_transition_rows_are_flagged` | PASS |
| T4 | Payload redaction, secret-shaped keys and token-shaped ids never leak | `payloads_are_redacted_before_they_reach_the_view`; `redaction_removes_secret_shaped_values_and_unknown_keys`; `sensitive_or_unrecognized_property_names_are_not_exposed`; `request_trace_is_operator_only_bounded_and_redacted` | `LIB payloads_are_redacted_before_they_reach_the_view`; `LIB redaction_removes_secret_shaped_values_and_unknown_keys`; `LIB sensitive_or_unrecognized_property_names_are_not_exposed`; `BIN factory-core request_trace_is_operator_only_bounded_and_redacted` | PASS |
| T5 | Event and step truncation are bounded and reported | `truncated_read_is_reported_and_bounded`; `step_truncation_is_reported_and_bounded`; `bounded_step_history_reports_truncation_and_keeps_the_newest_rows` | `LIB truncated_read_is_reported_and_bounded`; `LIB step_truncation_is_reported_and_bounded`; `LIB bounded_step_history_reports_truncation_and_keeps_the_newest_rows` | PASS |
| T6 | Real Core/Compose browser trace view, long production run and offline refresh | n/a (component/endpoint only) | WP10 browser acceptance | NOT_TESTED (browser/live) |

## Repeat-run policy

`scripts/quality-wave12-acceptance.sh` runs `provider_chaos` three consecutive
times. The candidate manifest records each run separately. The
flaky-sensitive integration suites (`planning_pipeline`, `provider_chaos`,
`dependency_broker_http`, `openspec_compose_validation`,
`migration_skill_refresh`) and the focused workflow/quality/trace/reviewer/Gate
filters were also repeated three times in the recorded run; all passed and no
retry was used to turn a failure green.

## Explicitly out of scope

- Paid provider inference, live OpenCode/Codex long-turn or SSE-cut UAT.
- Real rootless-Podman dependency acquisition, live registry fetch, real
  Compose dependency acquisition and the proxy path.
- A real Compose Supervisor restart at every crash point and HTTP inventory
  recovery.
- A real multi-reviewer Campaign with two or more role receipts.
- A real Core-backed OpenSpec projection produced by the model planning
  pipeline.
- Office per-option browser acceptance and visual review; WP10 owns the Office
  surfaces and browser harnesses.
- Independent verification, Quality Gate acceptance, deployment, Windows and
  `TRUSTED_LOCAL` isolation. No isolation or certification is claimed.
