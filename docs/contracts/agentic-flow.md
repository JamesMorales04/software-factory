# Agentic flow contract

**Status:** proposed for F1-S10. This contract is normative for the typed
agentic-flow records and the deterministic boundaries they must preserve. It
does not claim that every role or provider is implemented.

## Authority

Core owns identity, authority, budgets, risk, leases, candidate acceptance and
external effects. A model may propose intent, route, stack, work decomposition,
specifications, knowledge, findings or repairs. A model never grants authority,
selects a tool outside its compiled tool pack, widens scope, changes a frozen
specification, accepts a candidate or approves delivery.

## Request kinds

| Kind | Meaning | Mutation |
|---|---|---|
| `INFORMATIONAL` | Explanation or lookup | None |
| `AUDIT` | Read-only engineering assessment | None |
| `CHANGE` | Repository change | Governed route |
| `REPAIR` | Repair of an exact rejected candidate | Bounded change |

## Routes and assurance

Routes: `NO_AGENT`, `DIRECT`, `DELEGATED`, `FORMAL_SPEC`, `REPAIR`.
Assurance: `NONE`, `STANDARD`, `ELEVATED`, `CRITICAL`.

Route and assurance are separate. Core applies non-weakening floors from
observable facts. A semantic assessment may raise but never lower a floor.

## StackProfile

```text
languages, frameworks, runtimes, package_managers, build_tools, data,
test_frameworks, browser, infra, security_sensitive, conventions,
evidence_refs, confidence, unknowns
```

Deterministic manifests and file inventories are evidence. The model interprets
them. Unknown values remain explicit; they never become invented facts.

## ParticipantPlan

A deterministic compilation of:

```text
route, assurance, risk, work graph, stack, candidate impact, available roles,
specializations, tool packs, accepted skills, budgets, path conflicts
```

Each assignment binds role, specialization, provider/model route, session
identity, tool pack, skill manifests, allowed paths, output contract and budget.
Reviewers are read-only. Implementers and Repairers may write only inside their
lease. The plan cannot be widened by a model.

## Context policy

- Trigger compaction at 70% of the effective context.
- Prefer tool-result clearing and structured notes before compaction.
- At most two consecutive compactions before an audited handoff.
- Handoff at a stable milestone, or sooner if the summary is uncertain.
- Handoff summary carries deterministic identities and artifact references; it
  never replaces durable evidence.
- A new session verifies the handoff with probes before continuing.

## Specs and issue projection

Core owns the frozen spec. A deterministic Spec Publisher may create or update
GitHub issues and a kanban projection. Issue edits/comments are operator context;
they cannot mutate a frozen spec. A material change requires a new spec
revision. A local board is the fallback when GitHub is unavailable.

## Repair

A RepairPacket binds the exact rejected candidate, findings, failed checks,
allowed paths, risk and remaining budget. The Repairer returns a ChangeSet from
that candidate. Core creates C+1, invalidates C evidence and revalidates. R0 may
be autonomous under policy; R1 is bounded; R2/R3 and reserved effects follow the
owner charter.

## Owner charter

A durable owner charter defines domains, projects, risk appetite, budgets,
reserved matters, expiry and revocation. Execution, repair, delivery and policy
autonomy are separate. Credentials, isolation, production, destructive
migrations, public contracts, security exceptions, policy changes, out-of-budget
work and cross-project effects are reserved unless specifically delegated.

## Observability

Core records each participant's role, specialization, provider/model, session,
context/handoff, tools, skills, usage, latency, cost and decision reasons. No
private reasoning or secret is exposed. Telemetry never grants authority.
