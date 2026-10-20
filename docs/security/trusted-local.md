# TRUSTED_LOCAL operational policy

**Status:** Agreed owner policy per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md). TRUSTED_LOCAL is owner-authorized local execution with broad host access. It is **not** a sandbox, not isolated, and not certified. Historical SEC-01 FAIL/NOT TESTED findings remain valid observations; they do not block this mode.

## Scope

This policy governs TRUSTED_LOCAL runs only: real local Paseo/agent sessions operating within the owner's user privileges on the owner's machine. It does not govern ISOLATED or RESTRICTED execution, which keep their own evidence-bound gates (see [isolation policy](isolation.md)).

## Secret handling

- Never deliberately read, print, or exfiltrate real secrets (provider tokens, API keys, GitHub merge credentials, Core DB contents, other-project secrets, host auth files) beyond what the task strictly requires.
- No secrets in prompts, logs, telemetry, evidence bundles, commits, PRs, or screenshots. Use redaction at the log boundary per the applicable contract.
- Synthetic values only in tests: canaries and credential fixtures use fake values; real authentication materials never go into untrusted test code or disposable fixtures.
- If a secret is accidentally captured, stop, redact, rotate if owner-directed, and record the incident; do not commit the unredacted artifact.

## Host-operation limits

- No root escalation, no privilege-expansion flags (`--privileged`, host network, broad `HOME` or auth mounts, privileged socket mounts), no persistent background services, unless each is separately owner-authorized with actor, reason, expiry, and audit evidence.
- No destructive out-of-task operations (deleting data outside the task workspace, modifying other projects, changing system configuration, pushing delivery effects) without separate explicit authorization.
- Stay within the assigned workspace and task scope; treat every path, mount, and network destination outside it as requiring justification.
- A tool or sandbox failure fails closed: never silently continue with wider access to make a check pass.

## Transparency wording for Office and CLI

- Label every TRUSTED_LOCAL run exactly `TRUSTED_LOCAL — broad local access, not isolated`.
- Forbidden labels for this mode: `isolated`, `sandboxed`, `certified`, `SEC-01 PASS`, `credential-isolated`.
- User-journey surfaces (run detail, timeline, readiness output) must show the mode before execution starts and keep it in evidence records.
- Mode changes (ISOLATED to LOCAL, including TRUSTED_LOCAL) require explicit owner authorization recorded with actor, reason, and scope; silent downgrade blocks acceptance.

## Relationship to frozen contracts

This policy does not amend frozen S1/S2 contract semantics. The S2 synthetic-fixture contract (`run-worker-s2.md` v1) keeps its credential-free scope; TRUSTED_LOCAL workload kinds, if ever contracted, require a separate versioned revision. See the amendment note in [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md).
