---
name: factory-real-e2e
description: Exercise and record a bounded real Factory Core, Worker, provider and model engineering Campaign, then bind independent acceptance to its frozen candidate.
allowed-tools: Bash(python3 scripts/validate-live-certification.py:*) Bash(bash scripts/e2e-s4c-07-live.sh:*) Bash(bash scripts/e2e-s4c.sh:*) Bash(bash scripts/e2e-s4b.sh:*)
---

# Factory real engineering Campaign certification

Use this skill when a change affects Core orchestration, Worker/provider execution,
capability provisioning, agent workflows, quality integration, or execution
against an external project. It guides a bounded real product run and evidence
review. It does not itself grant a paid-session budget, external credentials,
delivery authority, or acceptance authority.

## Evidence boundaries

- Deterministic simulations and regressions establish only their deterministic
  claims. They cannot establish that a real provider session or model-mediated
  interaction occurred.
- `REAL_PROVIDER_E2E_VERIFIED` is bound to the Factory implementation SHA, frozen
  spec, Project, Campaign, Run/Attempt, any required provider/model session,
  frozen candidate and independent acceptance records. Apply browser, CLI,
  backend and recovery requirements from the frozen ActionSpec; do not infer
  them from a manifest label.
- Product Owner UAT is a separate owner attestation. It does not prove runtime
  security, deployment readiness, or SEC-01 isolation.
- The manifest validator checks repository-recorded structure and identity
  correlation. It cannot independently corroborate an external model session
  or the candidate Git object in an external Project repository.
- TRUSTED_LOCAL is broad host access. Never describe it as isolated or
  SEC-01-certified.

## Procedure

1. **Establish the candidate.** Record `git rev-parse HEAD` and
   `git status --short --branch`. The worktree must be clean before invoking a
   live lane. Record the exact SHA in the campaign evidence. Do not reset or
   clean unowned changes.
2. **Run the relevant deterministic lane first.** Use the documented acceptance
   script for the affected slice (for example `bash scripts/e2e-s4c.sh` or
   `bash scripts/e2e-s4b.sh`). Preserve its full exit status and evidence. A
   simulator pass is not live evidence.
3. **Check real provider/model availability without starting a session.** Use
   `provider provider ls` and `provider provider models <provider>` and compare the
   returned exact provider/model to the frozen ActionSpec. Do not silently
   select a fallback. If unavailable or unauthorized, record `LIVE_BLOCKED` or
   `LIVE_NOT_TESTED`; do not claim acceptance.
4. **Confirm standing authorization and remaining budget.** The existing
   bounded C-07 Vlog entrypoint requires `FACTORY_S4C_LIVE=1` and can consume
   paid sessions. GOV-01 authorizes zero new paid sessions. Do not set that flag
   for GOV-01 or infer authority from an earlier unused budget. For a future
   authorized C-07 run, use `FACTORY_S4C_LIVE=1 bash scripts/e2e-s4c-07-live.sh`;
   it starts real Core, Worker, verifier, reviewer and Office components,
   checks provider availability, and executes the Factory-managed Campaign.
   Never run a paid lane unless its exact budget and objective are authorized.
5. **Observe the real lifecycle.** Preserve the generated sanitized evidence
   file. Check that it identifies Project, Action, frozen ActionSpec/Campaign,
   Run, Attempt, Worker, any required provider workspace/session and
   provider/model/mode, and exact candidate SHA. Read the capabilities and
   executable acceptance criteria from the frozen ActionSpec. If it requires
   browser capability, confirm the actual Implementer model-mediated browser
   commands and managed server. CLI/backend Actions need their specified real
   command, build, API or service observations; they do not inherit browser
   requirements. A recovery claim needs a failed Attempt and a separate
   corrective Run with a newly accepted candidate.
6. **Require independent candidate acceptance.** Confirm that the verifier
   used a clean candidate checkout and passed the functional and capability
   oracles required by the frozen spec; the independent reviewer approved the
   same candidate/spec; and the Core Quality Gate accepted that candidate. A
   reviewer opinion cannot override a rejected gate. Preserve identities and
   status for each decision.
7. **Check lifecycle cleanup.** Verify the managed server stopped, its port is
   closed, the provider session is closed/archived, and no workspace remains
   active. Record unresolved cleanup as incomplete, not PASS. Do not kill
   processes outside the lane's managed resources.
8. **Freeze and validate.** Confirm the implementation SHA still matches the
   tested head. Create or update a manifest conforming to
   `docs/quality/certification-manifests.schema.json`, with a path to the raw
   sanitized source evidence. Install the pinned Python dependency and run:

   ```sh
   python3 -m pip install -r scripts/requirements-certification.txt
   python3 scripts/validate-live-certification.py docs/evidence/<manifest>.json
   ```

   The built-in positive example is the historical C-07 Vlog manifest. The
   validator output distinguishes schema validity and repository correlation
   from exact external candidate Git checks and provider observation. It is not
   independent proof of an external session. Retain independent runtime
   observation/review in the campaign record.
9. **Assess freshness.** Any change to the affected runtime after the tested
   implementation SHA invalidates the live result for that runtime. Reassess
   whether the change is documentation-only or affects execution; if affected,
   obtain a newly authorized bounded run. Never rewrite a historical result to
   the new SHA.
10. **Report dimensions separately.** Report deterministic verification, live
    provider execution, candidate-bound independent verification/review/gate,
    Product Owner UAT, deployment readiness, and security/isolation status
    separately. Include failures, retries, resource use, cleanup, and anything
    blocked or not tested. A real model failure remains in the record.

## Current historical example

The C-07 certification is specifically for Vlog on Factory head
`9567bc5c144235bef341524adc44277d377f6167`, runtime `14133cb7`, candidate
`e49f62df32c26e185c840b262435f7a21fea07fd`. It records one Implementer session,
model-mediated browser interaction on the initial turn, independent verifier
PASS, reviewer APPROVE and gate ACCEPTED. Corrective repair was not exercised.
This evidence does not certify deployment, autonomous GitHub delivery, or
SEC-01 isolation. See the raw source and normalized manifest under
`docs/evidence/`.
