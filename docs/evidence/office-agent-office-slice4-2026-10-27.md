# Agent Office and WorkGraph — Slice 4 evidence

**Candidate:** `2af7b68d504b21a33f47a5d586eef17bf4cefc72`
**Evidence run head:** `e76602885dcfefc41da030dab79f065b86f2c7a5`
**Base:** `0a03c7cd2bd6e6af52395c760c1b81ebe187a548`
**Status:** Implemented; independent review and Product Owner approval pending.
**Paid inference:** none.

## What was exercised

- Typed `GET /office/participants` projection over provider sessions, Lead
  requests/model calls, quality receipts and Worker registrations.
- Bounded `GET /provider/sessions/{id}/tool_events` projection over the existing
  durable `provider_tool_events` ledger, with allowlisted safe fields and
  withheld free-form provider detail.
- Agent Office visual floor, table and timeline with URL-backed filters and a
  participant inspector.
- Interactive Campaign WorkGraph with deterministic dependency layout,
  keyboard node navigation, exact recorded statuses and an accessible table.
- Cross-navigation from Agent Office to Campaign, WorkUnit, execution,
  candidate, evidence and Gate sections.

## Gate results

| Gate | Result |
| --- | --- |
| Rust format, Clippy (deny warnings), workspace tests, build, doc | PASS |
| `cargo xtask e2e recovery`, `dispatch`, `negatives`, `provider` | PASS |
| Architecture check against `origin/main` | PASS |
| Repository hygiene strict | PASS |
| Office typecheck, lint, 49 component tests, production build | PASS |
| Real Chromium host journey (`rust-journey.mjs`) | PASS |
| Deterministic Compose E2E | PASS (`ACCEPTED`, restart persistence) |
| Compose Agent Office/WorkGraph Chromium smoke | PASS |
| Compose policy and negative checker | PASS |

## Real-browser observations

- Agent Office rendered real Lead, Implementer, Verifier, Reviewer and Worker
  participants.
- An active provider-backed Implementer was distinguished from stale records;
  the session showed `ACTIVE`, `CURRENT`, live lease state and recorded tool
  events.
- Free-form provider explanation text was withheld as `WITHHELD`; no raw
  provider directory, prompt, source body or credential was rendered.
- Campaign WorkGraph rendered one recorded unit, supported node selection,
  opened the exact inspector and preserved `tab=workgraph&unit=...` across
  reload.
- Desktop 1440 px, tablet 834 px and mobile 390 px had no document overflow.

## Post-evidence code fix

Commits `4271d48` and `2af7b68` align the projection with the contract after
independent review: Lead liveness is `NOT_EXPOSED`, deep-linked participants
respect active filters, duplicate authoritative WorkGraph rows remain in the
table, quality-receipt ordering matches the global participant order, and
WorkGraph session selection prefers the latest generation. They also scope
quality-receipt role filtering in SQL, advance tool-event cursors across
malformed rows, and count non-positive legacy tool sequences as discarded.
Host Chromium and Compose E2E were run at `e766028`; the later commits are
read-projection/UI-ordering fixes covered by Rust/Office tests and Clippy, and
full exact-head CI is rerun on the final PR head.

## Limitations

- The Compose deterministic Campaign is API-created and has no WorkGraph units;
  the Compose WorkGraph check therefore asserts the truthful empty state.
- Multi-node, parallel, diamond and eight-unit graphs are covered by component
  and layout tests rather than a live provider multi-unit Compose run.
- No live paid provider UAT, SEC-01 certification, independent security review
  or Windows certification is claimed.

## Reproduction

See [Compose deployment](../../operations/compose-deployment.md) for the exact
commands. The screenshot is stored in this directory.
