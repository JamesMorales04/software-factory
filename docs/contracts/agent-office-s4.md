# Contract: Agent Office participant projection and interactive WorkGraph (F1-S4)

**Status:** Agreed for Factory Office Slice 4. **Version:** `1`. Experimental
pre-release contract; breaking changes update all in-repo callers, tests,
fixtures and documentation atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

This slice adds read-only Core projections and an Office experience. It adds no
agent runtime, scheduler, event ledger or execution authority. Factory Core
remains authoritative for Campaigns, WorkGraph tasks, Runs, Attempts, leases,
provider-session bindings, Worker registrations, quality receipts, candidates
and Quality Gate decisions.

## 1. Sources of truth

| Projection | Authoritative source | Relationship |
| --- | --- | --- |
| Implementer participant | `provider_sessions` | One participant per observed provider-session generation bound to an Attempt. |
| Implementer assignment | `attempts -> runs -> campaigns -> actions` | Exact recorded chain. |
| Implementer WorkUnit | `lead_tasks.attempt_id` | Legitimate binding when a task records the Attempt. |
| Implementer candidate/evidence | `candidates`, `gate_decisions`, `quality_receipts` | Joined by the exact Attempt and candidate. |
| Lead participant | `lead_requests` | A Core-recorded planning/assessment operation, not a continuous agent. |
| Lead route/usage | `model_call_reservations` role `LEAD` | Requested reservation route and measured usage of recorded calls. |
| Verifier participant | `quality_receipts` role `verifier` | Independent receipt with immutable principal and result. |
| Reviewer participant | `quality_receipts` role `reviewer` | Independent receipt including deterministic/model mechanism. |
| Worker participant | `worker_sessions` | Registration identity only; no heartbeat liveness is exposed. |
| Tool events | `provider_tool_events` | Existing durable provider tool-event ledger; no competing ledger. |
| WorkGraph | `CampaignDetail.tasks` (`lead_tasks`) | Frozen `unit_id`, `sequence`, `depends_on`, task status and execution links. |

Relationships not recorded in Core are reported as `NOT_EXPOSED`,
`NOT_APPLICABLE` or an equivalent explicit unknown. The projection never
synthesizes a persistent agent from a model name, never invents a Worker
assignment, and never turns a Lead model call into a continuously running
session.

## 2. Participant projection

`GET /office/participants` (operator Bearer token).

Query parameters:

| Parameter | Contract |
| --- | --- |
| `role` | Exact uppercase role: `IMPLEMENTER`, `LEAD`, `VERIFIER`, `REVIEWER` or `WORKER`. |
| `provider` | Exact, length-bounded provider string. |
| `status` | Exact uppercase recorded status from the provider-session, Lead, quality-receipt or Worker vocabulary. |
| `campaign_id` | Exact Campaign projection scope. |
| `project_id` | Exact Project projection scope; this is not an authorization boundary. |
| `participant_id` | Exact projected participant identity. |
| `limit` | `1..=200`, default `100`. |
| `offset` | `0..=10000`, default `0`. |

Invalid values return `400`. Missing or incorrect operator authentication
returns `401`; Worker, verifier and reviewer credentials are not operator
credentials. An unknown Participant identity returns an empty page, never a
synthesized record.

Response:

```json
{
  "participants": [
    {
      "participant_id": "implementer:<session_record_id>",
      "role": "IMPLEMENTER",
      "identity_kind": "PROVIDER_SESSION",
      "identity_id": "<session_record_id>",
      "principal_id": "recorded implementer or null",
      "project_id": "...",
      "action_id": "...",
      "campaign_id": "...",
      "request_id": null,
      "work_unit_id": "unit-a",
      "run_id": "...",
      "attempt_id": "...",
      "attempt_generation": 2,
      "worker_id": "...",
      "requested_route": {"provider": "opencode", "model": "...", "mode": "build"},
      "observed_route": null,
      "route_provenance": "PROVIDER_SESSION_BINDING_REQUESTED",
      "session": {
        "session_record_id": "...",
        "state": "ACTIVE",
        "provider_session_id": "...",
        "input_tokens": 100,
        "output_tokens": 50,
        "total_tokens": 150,
        "cost_micros": 25,
        "usage_known": true,
        "cleanup": "PENDING",
        "cleanup_receipt": null,
        "error": null,
        "created_at": "RFC3339 UTC",
        "updated_at": "RFC3339 UTC"
      },
      "status": "ACTIVE",
      "status_source": "PROVIDER_SESSION",
      "lease_state": "LIVE",
      "observation": "CURRENT",
      "failure_reason": null,
      "usage": {
        "input_tokens": 100,
        "output_tokens": 50,
        "total_tokens": 150,
        "cost_micros": 25,
        "usage_known": true
      },
      "usage_provenance": "PROVIDER_SESSION_BINDING",
      "evidence": null,
      "model_call_count": null,
      "model_call_state": null,
      "observation_note": "Core holds a live attempt lease; external provider state is not independently verified.",
      "recorded_at": "RFC3339 UTC",
      "last_observed_at": "RFC3339 UTC"
    }
  ],
  "page": {
    "limit": 100,
    "offset": 0,
    "returned": 1,
    "has_more": false,
    "order": "last_observed_at DESC, participant_id ASC",
    "as_of": "RFC3339 UTC"
  },
  "sources": [
    {
      "role": "IMPLEMENTER",
      "source": "provider_sessions -> attempts -> runs -> campaigns",
      "note": "One participant per observed provider session generation."
    }
  ]
}
```

Rules:

- `lease_state` is `LIVE` only when a `worker_leases` row matches the exact
  Attempt and session generation and has not expired. Otherwise it is
  `EXPIRED` or `RELEASED`.
- `observation` is `CURRENT` for a live lease, `STALE` for an unresolved
  session without a matching live lease, `FINAL` for a terminal recorded
  lifecycle, `NOT_EXPOSED` when Core does not record liveness, and `UNKNOWN`
  otherwise. `STALE` is not proof that an external session stopped.
- `requested_route` is the frozen session/model-call binding. `observed_route`
  is populated only for recorded model-review receipts; a provider session
  projection does not claim an observed route unless Core records one.
- Implementer usage comes from the session binding; Lead usage is the sum of
  recorded LEAD model-call usage; review usage comes from the receipt.
  Unmeasured usage is `usage_known=false`, never zero.
- Evidence joins require the exact candidate and, for gate outcome, the exact
  `gate_decisions` row. A task or candidate without a Gate is never labelled
  `ACCEPTED`.
- Ordering is deterministic: `last_observed_at DESC`, then
  `participant_id ASC`. Pagination is bounded and duplicate identities are
  collapsed deterministically.

## 3. Tool-event projection

`GET /provider/sessions/{session_record_id}/tool_events` (operator Bearer
token).

Query parameters:

| Parameter | Contract |
| --- | --- |
| `after_sequence` | `>= 0`, default `0`; strictly monotonic cursor. |
| `limit` | `1..=200`, default `100`. |

The exact session must exist (`404` otherwise). Response:

```json
{
  "session": {
    "participant_id": "implementer:<session_record_id>",
    "session_record_id": "...",
    "attempt_id": "...",
    "run_id": "...",
    "campaign_id": "...",
    "worker_id": "...",
    "generation": 1,
    "provider": "opencode",
    "model": "...",
    "provider_mode": "build",
    "state": "ACTIVE",
    "lease_state": "LIVE",
    "cleanup": "PENDING",
    "updated_at": "RFC3339 UTC"
  },
  "events": [
    {
      "sequence": 1,
      "call_ref": "16-hex-digest",
      "tool": "write_file",
      "tool_digest": "16-hex-digest",
      "decision": "REQUEST",
      "detail": "tool requested",
      "detail_state": "RECORDED",
      "recorded_at": "RFC3339 UTC",
      "provenance": "provider_tool_events"
    }
  ],
  "after_sequence": 0,
  "limit": 100,
  "returned": 1,
  "next_sequence": 1,
  "has_more": false,
  "discarded": 0,
  "source": "provider_tool_events",
  "note": "Bounded Core projection. Free-form provider detail is withheld unless it matches the Core allowlist; call and tool identities are digest references."
}
```

Safe-field rules:

- `call_ref` and `tool_digest` are stable SHA-256 digest prefixes. The raw
  provider call id is not returned.
- `tool` is returned only for a Core allowlist; other tool names become
  `OTHER` and retain only the digest.
- `decision` is one of `REQUEST`, `ALLOW`, `DENY`, `RESULT`, `ERROR`. A legacy
  or malformed decision becomes `UNKNOWN` with redacted metadata.
- `detail` is returned only for exact benign Core strings or a bounded
  `exit=<integer>` form. Final explanations, prompts, source bodies, command
  output, denial reasons and unrecognized legacy text are withheld:
  `detail_state` is `WITHHELD`, `REDACTED` or `EMPTY`.
- `provider_directory`, turn ids, credentials, request bodies and model
  responses are never returned. Error text is bounded and shape-redacted.
- Rows with non-positive or out-of-order sequences are discarded and counted.
  The projection never reorders or repairs the durable ledger.

`has_more` is computed with a bounded look-ahead. A client appends pages by
passing `after_sequence = next_sequence`; refetching with the same cursor is
idempotent by sequence. There is no SSE or WebSocket claim in this slice.

## 4. Office experience

Routes:

- `/agents` — visual office, table and timeline with URL-backed filters.
- `/agents/:participantId` — the same participant projection with the
  inspector selected and reconstructed from Core after reload.

Views share one typed participant projection. Project, Campaign, role,
provider and recorded-status filters are applied by Core. Project scope is a
projection filter, not an authorization boundary.

The inspector exposes role, provenance, Campaign/Action/WorkUnit/Run/Attempt,
generation, Worker, requested/observed route, provider session, cleanup, error,
measured/unmeasured usage, Candidate and independent receipt/Gate links, exact
timestamps and the bounded provider tool-event projection. A Lead shows no
session. A Worker shows registration identity only.

The UI does not add management actions, direct execution, stop, model switching,
budget changes or tool invocation.

## 5. Interactive WorkGraph

The Campaign WorkGraph remains sourced exclusively from
`CampaignDetail.tasks` (`lead_tasks`). The graph is a deterministic 2D layout
over frozen `sequence` and `depends_on`; the existing table remains visible as
the accessible authoritative alternative.

- Ready, waiting, blocked and inconsistent presentation states are derived
  from the exact recorded task status and recorded dependencies. They are
  labelled as derived and never presented as scheduling progress.
- Missing dependencies, self-dependencies, cycles, duplicate units, unknown
  statuses and sequence/dependency contradictions are surfaced as issues. The
  UI does not repair, omit or silently reorder them.
- Node selection is keyboard accessible. Arrow keys move between nodes; Enter
  or Space selects. Selection is stored as `?tab=workgraph&unit=<unit_id>` so
  reload and browser history restore the exact context.
- A candidate is not labelled `ACCEPTED` unless the exact Core Quality Gate
  decision for that candidate is `ACCEPTED`.
- The node inspector links to the exact Campaign execution, candidate,
  evidence and Gate sections and to the correlated Implementer participant
  when a provider session is genuinely bound.

## 6. Authorization and security

- Every endpoint checks the exact operator Bearer token server-side. Worker,
  verifier and reviewer credentials are rejected.
- Worker credentials are never accepted as operator credentials, and operator
  credentials never grant Worker execution.
- The browser never reads SQLite, controls Workers, reads provider credentials
  or writes Core state.
- Displayed values come from Core records or are labelled
  `NOT_EXPOSED`/`UNKNOWN`. The projection does not infer external provider
  liveness, progress or acceptance.
- `TRUSTED_LOCAL` is never labelled isolated, sandboxed or certified.

## 7. Verification

Required negative and boundary coverage:

- missing, incorrect, Worker, verifier and reviewer authentication;
- invalid role, status, provider, limit, offset and cursor filters;
- cross-Campaign identity confusion and unknown participant/session identity;
- superseded lease generation and stale/orphaned cleanup;
- missing participant identity and missing provider session;
- malformed or legacy tool-event rows and sensitive-detail redaction;
- large histories, bounded pagination, duplicate collapse and deterministic
  ordering;
- empty, single-node, linear, parallel, diamond and eight-unit WorkGraphs;
- blocked dependency, failed Attempt, missing candidate, candidate without a
  Gate, accepted candidate with a bound Gate, stale projection and malformed or
  inconsistent graph data;
- real Chromium Office and Campaign WorkGraph journeys against the real Core.

## 8. Non-goals

This contract does not implement an agent runtime, SSE/WebSocket streaming, a
new provider integration, an autonomous scheduler, SEC-01 isolation, Windows
certification, GitHub Issues, Docs or direct management actions from Agent
Office.
