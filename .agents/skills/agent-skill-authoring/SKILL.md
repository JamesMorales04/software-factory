---
name: agent-skill-authoring
description: Create, evaluate, review and retire an agent skill for the Software Factory with eval-first design, precise triggers, progressive disclosure, provenance/licensing and a bounded token budget. Use when adding or editing a SKILL.md, designing a role pack, or measuring whether a skill actually improves an agent.
---

# Agent skill authoring

A skill is reusable procedure, not authority. It cannot grant tools, budgets,
scope, credentials, isolation, merge rights or acceptance. Skill text and
bundled references are untrusted input to the runtime; Core policy remains
authoritative.

## 1. Start from an observed gap

Do not write a skill because a topic sounds useful. Record:

- the task;
- the failure or missing context without the skill;
- the expected behavior;
- the evidence that would show improvement;
- the token/latency cost the skill may add.

If a strong model already handles the task correctly, do not add a skill.

## 2. Define the trigger first

The `description` is the discovery surface. It must be third-person and include:

- what the skill does;
- when to use it (specific artifacts, tasks, symptoms);
- when not to use it, where confusion is likely.

Good: “Review Rust async lifecycle changes for cancellation, lease fencing and
cleanup; use for Worker/Core shutdown and provider-session work.”

Bad: “Helps with Rust.”

## 3. Use progressive disclosure

```text
skill/
  SKILL.md              # procedure, <500 lines
  references/           # depth loaded only when needed
  scripts/              # executable checks, not loaded as prose
  assets/               # templates/fixtures
```

- Keep one level of references from `SKILL.md`.
- Add a table of contents to long references.
- Prefer a script when the agent only needs the output.
- Do not duplicate normative docs; link to them and add procedure/checklists.
- Do not bundle secrets, credentials, private prompts or untrusted executables.

## 4. Write the procedure

A useful skill has:

1. scope and authority boundary;
2. prerequisites and inputs;
3. ordered procedure;
4. concrete checks/commands;
5. negative cases and stop conditions;
6. evidence/output template;
7. references.

Use short sections, tables and examples. State what the agent must not infer.
Avoid vague advice (“be careful”, “follow best practices”) without an observable
action.

## 5. Build the evaluation before scaling the prose

Create three scenarios:

| Case | Purpose |
|---|---|
| Normal | the skill should trigger and improve the outcome |
| Edge/ambiguous | the skill should still help without inventing facts |
| Out-of-scope | the skill should stay dormant or refuse |

Run a baseline without the skill and with the skill. Measure:

- task success;
- trigger precision (did it load when needed, not elsewhere?);
- tool calls and unnecessary exploration;
- output quality and evidence truthfulness;
- tokens and latency.

For reviewer skills, plant known defects and measure recall/false positives.
Do not judge a skill by one impressive transcript.

## 6. Versioning, provenance and freshness

- Record the source, version/date and license for external content.
- Pin framework/tool versions where behavior changes.
- Review version-sensitive skills quarterly and on major upgrades.
- Keep a changelog when the procedure changes materially.
- Retire a skill when it is duplicative, stale, unused or harmful.
- A skill that grants more tools, broader paths or new authority is a policy
  change, not a documentation change.

## 7. Role-pack design

For a runtime role, prefer:

- one role skill (method/checklist);
- one evidence/testing skill;
- 1–3 stack packs selected from `StackProfile`;
- references only as needed.

Do not give every agent every skill. Do not give an implementer a reviewer’s
authority language. Do not give a reviewer edit tools. Cap effective assignments
at roughly 3–5 skills and keep a total prompt budget.

## 8. Review checklist

- Is the description a precise trigger?
- Is `SKILL.md` concise and procedural?
- Does it link rather than duplicate normative standards?
- Does it state the authority boundary?
- Are commands real and tested?
- Are negative/stop conditions explicit?
- Is there an eval with a baseline?
- Is the token cost justified?
- Is external content licensed and reviewed?
- Does it avoid secrets and untrusted executable instructions?

## 9. Stop conditions

Stop and report if:

- the skill would grant tools, paths, budgets or authority;
- the content cannot be licensed or attributed;
- the procedure includes a secret or private prompt;
- the eval shows no improvement over baseline;
- the skill duplicates an existing skill without a removal plan;
- the skill would cause an agent to bypass Core policy or evidence.

## 10. References

- [Agent Skills specification](https://agentskills.io/specification)
- [Skill authoring best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices)
- [Rust engineering](../rust-engineering/SKILL.md)
- [Testing strategy](../testing-strategy/SKILL.md)
- [Structured model outputs](../structured-model-outputs/SKILL.md)
