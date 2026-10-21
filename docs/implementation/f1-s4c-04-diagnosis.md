# F1-S4-C-04 — Browser capability completion diagnosis and preflight

**Status:** Diagnosis complete; same-session completion implemented and
deterministically verified. No paid Paseo session was spent for C-04.
Final live certification for the C-04 implementation remained
**HUMAN_REQUIRED** pending Product Owner authorization. F1-S4-C-07 later
achieved the bounded Vlog live path on tested head `9567bc5`; see
[C-07 live certification](f1-s4c-07-live-certification.md). This diagnosis is
historical and is not rewritten.

## Phase A — diagnosed root cause

Evidence sources: persisted OpenCode session/message/part records, Paseo
workspaces/sessions, S4-C live JSON, worker logs and the final candidate.

### Earlier success (`11cd9e6`, accepted Vlog trial)

The OpenCode session read `.agents/skills/factory-browser/SKILL.md`, inspected
the generated wrapper, started the app, and then explicitly ran the wrapper:
`open http://127.0.0.1:4173/`, `eval`, `console`, `requests`, `click`,
`snapshot`, `click`, `close`. The skill file and wrapper were present and
reachable; no permission or discovery error occurred. The successful commands
were recorded and the gate accepted the candidate.

### Final failure (`cfa250e`, Vlog `REPAIR_REQUIRED`)

The final OpenCode session also read the generated skill, implemented the
filter, ran format/tests, and started `PORT=4173 npm start` with logs under
`/tmp`. It then ended its turn without invoking the wrapper. There was no tool
error, no provider error, no timeout and no authorization failure in the
session. The Factory closed the session after the first productive turn; no
continuation check existed for a required-but-omitted browser interaction.

### Answers to the specific questions

- **Why earlier success but later inconsistency?** Model turn behavior varied.
  The earlier agent chose to use the skill/wrapper; the final agent considered
  the task complete after code + unit tests + server startup. The capability
  was available in both sessions.
- **Does OpenCode discover/load the skill?** The generated
  `.agents/skills/factory-browser/SKILL.md` was present and was read by the
  actual OpenCode agent in the live sessions. Automatic skill *registration*
  was not independently proven; the prompt directs the agent to the file and
  the agent read it.
- **Is the wrapper reachable/executable?** Yes. It was executed successfully
  in the earlier live session and independently preflighted later. The final
  session never attempted it.
- **Are instructions/endpoints delivered?** Yes. The prompt contains the
  wrapper path, mandatory `open` + interaction requirement and authorized
  loopback endpoints; the generated skill repeats them.
- **Ignored, errored, or out of time?** The final model ignored/finished
  early; no tool error or time exhaustion is evidenced.
- **Why one attempt and no automatic repair?** The final Vlog run was
  deliberately configured with `maxImplementerAttempts=1`,
  `maxRepairCycles=0` to stay within the last remaining C-03 Vlog session.
  The runner passes these values correctly. The broader authorization had
  already been consumed by earlier trials, so no repair cycle was possible.
- **Did the runner preserve lineage/budgets?** Yes. Run/session identities were
  recorded; candidate and independent verifier evidence were preserved. The
  failure was at the capability gate, not lineage.

## Phase B — corrective implementation

After a productive Implementer turn with a valid diff, the Worker now checks
for successful Factory-managed browser interaction before closing the Paseo
session. If the frozen Run requires `browser` and evidence is missing:

1. one bounded continuation is sent to the **same** Paseo session with the
   authorized endpoint, wrapper path and a mandatory `open` + interaction
   instruction;
2. the session is re-observed and evidence re-collected;
3. candidate freeze/verification proceeds with truthful evidence;
4. if the interaction is still missing or commands failed, the gate rejects;
   repair/recovery follows the existing bounded campaign paths.

The continuation does not create a new Run, Attempt, lease or paid session.
It is capped at one capability continuation per Run (plus the pre-existing
single empty-diff continuation; total at most two bounded nudges). Failed
commands, `--help` and unconfirmed cleanup never count as success.

## Phase C — certification semantics proposal

Proposed distinction (not a retroactive contract change):

- **Product browser acceptance** — independent verifier runs the protected
  browser oracle against the exact candidate in a clean checkout. This proves
  UI functionality.
- **Agent browser-capability certification** — the Factory-managed Implementer
  produced successful, model-mediated `open` + observation/interaction commands
  in the authorized Paseo session/Run. This proves capability use, not product
  correctness by itself.

C-04 preserves both requirements for this campaign. A future ADR may allow
projects to require only product acceptance; it must not rewrite the frozen
Vlog contract or prior decisions.

## Phase D — deterministic proof

The S4-C lane adds:

- **D1**: `omit-browser` productive Implementer receives one same-session
  capability continuation, completes `open,snapshot,close`, and is accepted
  with exactly one attempt and no recovery.
- **D2**: `browser-fails` command attempts are recorded as failed, the
  capability check remains incomplete, and the candidate is `REPAIR_REQUIRED`
  rather than accepted.
- R1–R8, C1–C18 regressions remain green.
- Independent clean-checkout verification: PASS on code candidate `0db2262`.
- GPT-6 Luna read-only review: `APPROVE_WITH_NOTES` on `0db2262`; the notes
  (bounded two-nudge maximum and command-name vs semantic interaction) are
  documented limitations, not gate weakenings.

## Phase E — live preflight and authorization gate

- Final code candidate: `0db2262`; final PR head `6a7fb67` adds tests and
  documentation only (same runtime).
- Skill: generated, readable and actually read by OpenCode in live sessions.
- Wrapper: real successful use observed in an earlier live session; preflight
  confirms executable and URL guard.
- Same-session continuation: deterministic D1 PASS; no paid session spent.
- Campaign budgets: live lane reads `S4C_LIVE_ATTEMPTS` and `S4C_LIVE_REPAIRS`;
  the final Vlog C-03 run used `1/0`.
- Previous live evidence is valid only for its recorded SHAs; it is stale for
  the final C-04 runtime. Calculator's remaining authorization is untouched.
- Non-paid preflight completed: real Core + real Worker registered with
  capabilities `repository-edit,node-toolchain,generic-toolchain,browser` and
  `trusted-local`; Vlog Project/spec/campaign created in `pending` with
  `maxAttempts=1`, `maxRepairCycles=0`; zero paid sessions started. The
  campaign was not started.
- Proposed bounded live run: **one** Vlog Implementer session with
  `maxImplementerAttempts=1`, `maxRepairCycles=0`, expecting the same-session
  continuation to complete the capability. If it fails, stop and report; do not
  consume the Calculator session or silently create a new allowance.
- **C-05 result (2026-10-21 UTC):** one authorized Vlog session was run on
  `65ca3a8`. The model produced candidate `3aa64273…` and independent browser
  PASS, but only `--help` was recorded. The C-04 same-session continuation was
  triggered (`capabilityNudges=1`) and the model attempted to restart the app,
  but the shell command errored and the agent ended without `open`/interaction.
  Gate `REPAIR_REQUIRED`. Session/workspace closed and archived. No further
  paid session was authorized for that implementation; C-07 later achieved the
  bounded Vlog live path on `9567bc5`.
- No additional paid session will be started without explicit Product Owner
  authorization.
