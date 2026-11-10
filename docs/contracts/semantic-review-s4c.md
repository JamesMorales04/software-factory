# Contract: Independent semantic review (S4-C)

**Status:** Implemented on the semantic-reviewer branch; final candidate
acceptance is recorded by the Director. Experimental pre-release contract.

S4-C adds an optional model-backed semantic Reviewer on the existing
Core-owned model-call gateway. The deterministic Reviewer remains the default.
The semantic Reviewer never holds provider credentials, never edits the
candidate, and never grants authority; the deterministic Quality Gate remains
the final decision mechanism.

## Mechanism

`FACTORY_REVIEW_MECHANISM=DETERMINISTIC` (default) runs the existing bounded
static review. `FACTORY_REVIEW_MECHANISM=MODEL` first runs that deterministic
pre-review. A deterministic `request_changes` result is final and no model call
is made. Only an approved pre-review proceeds to build a bounded bundle and
call `POST /model/calls` with role `semantic_reviewer`, `objectiveId` and
`scopeId` equal to the candidate ID, and purpose `semantic_review`.

A model `approve` is corroborating advice, not acceptance authority. The final
`PASS` receipt requires **both** a deterministic `approve` and a model
`approve`. A model or deterministic `request_changes` yields
`CHANGES_REQUESTED`; model uncertainty yields `NOT_TESTED`. This preserves the
invariant that model output never grants Quality Gate acceptance.

The model call is reserved and settled by Core. The response supplies
`reservationId`, `provider`, `model`, token usage, cost and `usageKnown`.

## Strict output

The decoded object is closed (`additionalProperties: false`), rejects unknown
fields, and binds the exact `candidateCommitSha`, `candidateTreeSha` and
`actionSpecSha256`. It contains:

- `verdict`: `approve | request_changes | uncertain`;
- bounded `summary`;
- `confidence`: `0..=100`;
- at most 20 bounded findings with unique `id`, `severity`
  (`blocking | warning | info`), `category`
  (`functional | architecture | security | testing | scope | other`),
  `message` and 1..=16 `evidenceRefs`.

Duplicate finding IDs, empty/oversized fields, a wrong candidate identity, too
many findings, and a blocking or critical finding combined with `approve` are
rejected. Every finding must set `points` to the canonical severity weight
(`critical=8`, `blocking=5`, `warning=2`, `info=1`) and include a non-empty
`rationale` plus 1..=16 evidence references. Core rejects any mismatch;
finding points are advisory input and never grant repair or acceptance.
Malformed or uncertain output is never converted into approval.

Informational (`info`) findings on a passing, executed reviewer receipt are
observations. They remain durable in `findings_json` and the finding ledger and
stay visible in Office, but they do not block an otherwise accepted Gate and
never produce or authorize a repair cycle. Warning, blocking and critical
findings keep their open-disposition and repair rules, and non-passing receipts
keep the existing finding behavior.

## Verdict mapping

| Semantic verdict | Receipt result | Gate effect |
| --- | --- | --- |
| `approve` with deterministic `approve` | `PASS` | May contribute to `ACCEPTED` |
| `approve` without deterministic `approve` | `CHANGES_REQUESTED` | `REPAIR_REQUIRED` |
| `request_changes` | `CHANGES_REQUESTED` | `REPAIR_REQUIRED` |
| `uncertain` | `NOT_TESTED` | Never `ACCEPTED` |
| rejected/malformed output | `FAIL` | `REPAIR_REQUIRED` |
| timeout / unknown usage / Core or upstream error | `NOT_TESTED` | Never `ACCEPTED` |

Failure receipts carry at least one concrete finding and model provenance when
Core returned it.

## Review bundle

The bundle contains the frozen objective and ActionSpec hash, exact candidate
commit/tree, changed paths, a bounded diff, bounded source excerpts read directly from
Git objects of the exact candidate (no symlinks or gitlinks, secret-shaped
names excluded, at most 128 KiB of source), verifier result/evidence,
acceptance and negative checks, and generic architecture constraints. Raw
prompts and source bodies are not persisted in receipts or telemetry.

## Durable receipt

Migration `0017_semantic_review.sql` extends `quality_receipts` with
`findings_json`, `mechanism`, `model_provider`, `model_name`,
`model_reservation_id`, token totals, `cost_micros` and `usage_known`.

Deterministic receipts use explicit defaults. The candidate's frozen Execution
Profile `semanticReviewer` route is the only reviewer selection authority:
`GET /quality/candidates/{id}/review-config` resolves it to a chat-completions
(`MODEL`) or native adapter (`CODEX_APP_SERVER`) transport. A global
`FACTORY_ENABLE_CODEX_REVIEW` flag is an availability signal only; it never
overrides a selected route. A `MODEL` `PASS` receipt is rejected unless it
matches a `SETTLED` `SEMANTIC_REVIEWER` reservation for the same candidate
scope, principal, frozen provider/model and measured usage. A native `PASS`
receipt is rejected unless its provider/model match the frozen route, it has no
Core reservation, it carries known measured usage and a request hash. Operator,
verifier and reviewer credentials may read candidate receipts for inspection;
ingestion remains role-scoped and independent.

Critical findings produce `HUMAN_REQUIRED` and cannot be converted into an
autonomous repair. For non-critical reviewer changes, Core recomputes points,
checks reviewer independence and applies the frozen `AutonomyPolicy` risk
budgets; the resulting `AUTHORIZED`, `DENIED` or `HUMAN_REQUIRED` decision is
append-only `autonomy_repairs` evidence. It is not delivery authority and does
not by itself create a new attempt. See ADR-0033.
