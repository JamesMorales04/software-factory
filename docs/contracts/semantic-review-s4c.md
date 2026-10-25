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
many findings, and a blocking finding combined with `approve` are rejected.
Malformed or uncertain output is never converted into approval.

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

Deterministic receipts use explicit defaults. A `MODEL` `PASS` receipt is
rejected unless it matches a `SETTLED` `SEMANTIC_REVIEWER` reservation for the
same candidate scope, principal, provider/model and measured usage. Operator,
verifier and reviewer credentials may read candidate receipts for inspection;
ingestion remains role-scoped and independent.
