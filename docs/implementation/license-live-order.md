# Exact licence live order and evidence checklist

Status: runbook for the authorized UAT. No call/build is made by this document.

## Preconditions

- Backend final SHA reviewed by Reviewer70, including the private
  `VerifiedSourceEvidence` and producer guards.
- Integrated candidate checks green; only invalidated checks rerun after a
  repair.
- Retained stack volumes, `18893`, `0fa4990e` evidence, intake `fc7e7311…`, the
  two settled calls and admission `5d6de67e…` preserved.
- Fresh Core-issued envelope/reservation/admission exists for the new plan; no
  R3 reuse, no SQL/chat reservation, no static grant.

## Serial build

Build the relevant images against the frozen integrated candidate, one at a
time, recording image IDs and logs:

1. Core/Office image (multi-stage `deploy/Containerfile --target core`).
2. Environment builder image (`deploy/environment-builder/Containerfile`).
3. Runtime broker image (`deploy/Containerfile --target runtime`) because the
   broker carries the licence-decision overlay.

Do not build in parallel. A failed build stops the sequence; do not reuse an
older image under the new tag.

## Restart on 18893

- Use a new compose copy tagged with the candidate image IDs and the **same
  Compose project name** so named volumes are preserved.
- `podman compose up -d` only after the new images exist; verify `/health`,
  `/observability`, the retained intake and the two settled calls.
- Never point the stack at `8893`, a host Podman socket, host networking or a
  different volume.

## Live order (Reviewer70-confirmed option a)

1. `POST /agentic/requests/fc7e7311…/environment/recompile` with a bounded
   rationale (no model). New plan revision/hash with the current catalogue.
2. `POST /agentic/requests/fc7e7311…/environment/review` bound to the new
   plan hash (new `lead_environment_review` call). The licence gate refuses,
   but Core persists the new plan/spec identity facts; the plan stays
   retryable.
3. `POST /agentic/requests/fc7e7311…/license-evidence-review` with the approved
   delegation id and an idempotency key (new `license_evidence_review` call).
4. `POST /agentic/requests/fc7e7311…/license-decision` with the same delegation
   and a reservation-derived idempotency key (no model).
5. Retry `POST /environment/review` with the same new plan hash. It reuses the
   recorded approved review (no third model call) and builds with the decision
   overlay; the spec freezes.
6. Continue the original B+CR15 E2E through the Quality Gate and delivery
   boundaries.

Two new Lead-route calls, not three. The retry is not a model call. Source
metadata needs no model. Record the exact purposes, provider/model, usage,
cost, reservation ids, request/output hashes and total real count (four total
if both new calls execute; never claim it before execution).

## Failure handling

- `LICENSE_IDENTITY_FACTS_REQUIRED` before the licence review means the new
  plan receipt is absent; run step 2, do not fabricate a receipt.
- `LICENSE_PLAN_STALE`/`LICENSE_BINDING_INVALID` means the plan/policy stage is
  stale; recompile, do not reuse the old plan or static asset.
- `LICENSE_EVIDENCE_MODEL_UNAVAILABLE`/`LICENSE_EVIDENCE_REVIEW_INVALID`
  settles no review; no ACCEPT and no decision.
- A terminal lead, withdrawn request, revoked delegation or altered delegation
  evidence is a no-call failure.
- Preserve the old review, plan hash, intake, admission and Owner declaration.
