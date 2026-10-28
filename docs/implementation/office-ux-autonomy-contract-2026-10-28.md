# Governed autonomous issue intake — proposed contract

**Action:** Wave 2 / #39, independent design only. **Inspected base:**
`924e026b8c31fb416238ea9998e55c3a45701bb3`. Only this document is authorized
for change. This is a proposal, not authorization to enable execution.
Implementation changes authority and is protected R3; eligible work remains R0.
Acceptance here means a reviewable design of at most 1600 words, committed on
this branch. No inference, provider spending, deployment or merge is authorized.

## Observed foundation and blockers

`intake.rs` treats AUTONOMOUS as intent, not start authority. `db/intake.rs`
creates bounded non-autonomous execution-policy grants. `db/lead.rs` implements
ADR-0029's shared start, withdrawals, expected bindings and Campaign execution
bindings, but explicitly rejects autonomous start of profile-bound requests.
`ValidatedAutonomyContext` currently proves only deployment enablement.

GitHub synchronization is bounded and read-only, with credential revisions,
backoff, snapshots and truncation. `GitHubIssue` has an issue ID and author login,
but lacks immutable author/repository IDs and author association. Issues Office
shows snapshots and plain-text bodies; it does not authorize execution.
Cross-role executability and aggregate monetary accounting must be verified on
the integrated R/P/intake baseline. Contract prose is not implementation evidence.

## Default-off policy and preauthorization

Propose Core-owned SQLite project policy revisions, append-only audit events,
and immutable grant-to-policy bindings, using versioned migrations. Absence means
disabled. Both deployment enablement and project enablement are necessary;
neither is sufficient. Profile `autoStart` configuration never grants authority.

Preauthorization means an authenticated owner explicitly acknowledges one finite
batch of exact issue snapshots and bounds before assessment. It replaces later
per-plan approval only within those bounds. First delivery excludes open-ended
subscriptions to future issues. A new issue, edited snapshot or enlarged batch
requires a new explicit grant; labels alone never authorize anything.

The grant acknowledgement displays repository, issues/digests, profile revision,
paths, capabilities, risk, execution mode, expiry and total possible spend.
Defaults are resolved once into the acknowledgement, never dynamically at start.
Policy edits disable old grants until explicitly reissued, without replenishing
old reservations. Audit enable/edit/disable/revoke, principal, reason, hashes,
capacity decisions and replay identities; never log credentials or issue bodies.

## Eligibility and decision mechanisms

**DETERMINISTIC** checks run before any paid assessment, at freeze/start and
before new dispatch. Require all of:

- Exact registered project, immutable GitHub repository ID, canonical remote,
  approved base commit/config hash and active connection revision.
- An acknowledged issue ID/content digest in the batch; open, unlocked,
  non-PR, untruncated content; exact required labels and no exclusion labels.
- Immutable author ID in an owner-selected allowlist and current association
  in OWNER/MEMBER/COLLABORATOR. Unknown identity/association fails closed.
- Exact executable profile revision/hash and role routes; active R0 autonomous
  delegation; allowed paths/capabilities intersect project, grant and plan.
  Protected policy, credentials, authorization, sandbox and delivery paths are
  excluded regardless of labels. No arbitrary commands supplied by issues.
- Positive remaining request/session/attempt/token/cost/time capacity, including
  assessment and mandatory review. Unknown usage stays reserved.
- No prior request for the same project/repository/issue, active conflicting work,
  unresolved dependencies, failed required base checks or stale base/config.
  First slice serializes project execution and requires an owner-acknowledged
  dependency set; ambiguous dependencies require intervention. Reopened issues
  cannot silently rerun. New scope requires explicit replacement authorization.

**MODEL**: reuse the bounded Lead assessment to propose semantic class, risk,
paths and dependencies through typed schema output. **HYBRID**: Core validates
that proposal against the acknowledged R0 objective and deterministic constraints;
uncertainty or suspected protected scope blocks for human review. Model risk
labels cannot lower risk or establish explicit R0 authority. Keyword heuristics
cannot certify semantic safety; final diff review must detect escalation and
stop execution/delivery. Policy checks are deterministic for reliability and
cost; bounded semantic assessment handles open-ended meaning without authority.

## Authorization envelope and existing lifecycle

An autonomous-mode grant is required, separate from the existing manual policy
grant: issue an immutable standing delegation with `autonomous_start=true`, R0
only, linked to the acknowledged batch/policy. Do not mutate a manual grant.

The canonical versioned envelope binds: grant/audit/principal IDs; project and
repository IDs plus binding revision; issue ID/number/content digest and observed
GitHub revision; connection revision; policy revision/hash; profile revision/hash
and all role routes; config hash/base commit; objective class/R0 attestation;
paths/capabilities/tools/egress/execution mode; expiry; per-call and aggregate
budgets; batch count/concurrency; dependency/base-check evidence; idempotency key.
At freeze add request, proposal, plan and executable-spec hashes. Hash canonical
bytes and persist immutable provenance with start evidence/execution bindings.

Extend ADR-0029's `Autonomous(ValidatedAutonomyContext)` internally to carry the
validated envelope, never deserialize authority from HTTP/model data. Preserve
`Operator` unchanged. Replace the profile-bound rejection only for this validated
case; deployment flag plus generic delegation remains insufficient for issue
intake. Every alternate start/attempt path must enforce the same provenance.

Atomically link one queue item to the existing intake draft Action and Lead
request, reserve assessment capacity, assess, freeze the first ActionSpec, then
use the shared transactional start to create the ordinary Campaign/Run/Attempt.
Start rechecks the envelope and reserves remaining bounds in the transaction.
Worker leases, independent Verifier/Reviewer and deterministic Quality Gate
remain authoritative. No second execution scheduler or acceptance authority;
intake status still derives from Lead/Campaign/gate/withdrawal. No automatic PR
write, merge or delivery authority is included.

## Queue, freshness and revocation

Add only a durable Core ingestion queue: unique project/repository/issue identity,
snapshot digest, grant revision, request link, bounded lease/retry timestamps and
reason. Dispatch through existing Core lifecycle services, in-process; no new
service, container or host dependency. Proposed initial ceilings: 20 issues per
grant, one active request per project, three transient read retries. Honor shared
credential Retry-After/backoff; exhausted limits require intervention.

Fetch each selected issue directly before assessment and start, including stable
identity/association and current labels/state. Require a successful observation
within 60 seconds; offline, partial/truncated sync, 401/403/404, changed content,
redirect or identity mismatch blocks. Compare connection/binding revisions again
at commit. GitHub cannot provide a transaction with SQLite: record observation
age and recheck before dispatch; never claim instantaneous external consistency.

Queue leases expire on restart; recover committed request links before retrying.
Exact replay returns original IDs without new spend, even after revocation;
replay never resumes execution. Unknown upstream outcomes remain charged and
blocked. Changed payload/key conflicts. Project/repository namespaces apply to
all lookups, uniqueness, cache entries, reservations and credentials; never
resolve by issue number or login alone.

Revocation/disable and start serialize through the shared write transaction.
Revoke the linked delegation, withdraw pending Lead requests, fence new leases
and provider calls, and request cancellation of active Campaign sessions using
existing containment. In-flight external work may finish: retain charges/evidence,
report cancellation pending, and forbid replacement dispatch. Credential rotation,
repository rebinding or scope loss invalidates pending observations/grants.

## Untrusted data and Office contract

Issue text, titles, comments and linked documents are untrusted data, never
instructions, policy or authority. Do not fetch links or attachments implicitly.
Keep content delimited from system instructions; model output is strictly decoded
and validated. Workers receive only authorized repository data and bounded tools;
GitHub/provider credentials remain behind existing Core/broker boundaries.
Deny arbitrary network destinations, host mounts, secret reads and upload tools.
Prompt filtering alone is insufficient; unavailable enforced confinement blocks
execution. TRUSTED_LOCAL requires separate recorded owner risk acceptance and
must never be described as certified isolation; ISOLATED remains fail-closed.

Future Issues/settings surfaces must expose acknowledgement, disable/revoke,
eligibility reasons, remaining budget, stale/offline/permission/backoff recovery,
and request/Campaign/evidence links. Require authenticated Core operations,
confirmation, reload-safe state, EN/ES and keyboard/mobile acceptance. Update
parity matrix, registry and roadmap with implementation; this document ships no UI.

## Required gates and first three commits

Before implementation, freeze test oracles: default-off/missing grant; forged
actor/model authority; new/edited/closed/excluded/untrusted-author issues;
injection/exfiltration/protected-path proposals; duplicate sync/reopen/replay;
start/revoke/withdraw races; revoked/rotated credentials; stale snapshots;
exhausted/unknown usage; cross-project/repository collisions; crash/restart at
reservation, intake and start commits; migration integrity and existing manual
approval regressions. Assert zero unauthorized provider calls and no duplicate
Campaigns or charges. Require real Compose/browser journeys and independent
candidate-bound review. Real authorized issue run is **NOT_TESTED** here; absent
credentials or fresh spending authorization remains NOT_TESTED/BLOCKED.

1. `feat(intake): persist default-off issue policy and bounded batch grants`:
   protected ADR approval first; migrations, immutable identities, audit,
   authenticated acknowledgement/revoke and Office configuration with tests.
2. `feat(intake): reconcile eligible issues into durable Lead proposals`:
   direct freshness checks, queue/recovery, advisory assessment, aggregate
   reservations, Office diagnostics; execution still disabled.
3. `feat(auth): start preauthorized issue Campaigns through shared Lead boundary`:
   validated envelope, atomic fencing/replay, lifecycle/browser/adversarial tests,
   fresh owner-authorized live run and independent frozen-candidate verification.

A new protected ADR must explicitly amend ADR-0029's profile-intake restriction
and standing-autonomy issuer/risk policy; reconcile shared contracts and MASTER.
Owner authority, independent architecture/security review and branch protections
are prerequisites to enabling or merging this R3 change. This design grants none.
