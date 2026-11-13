# Codex Luna review fleet

**Status:** development harness guidance. Not product runtime, not independent
acceptance and not an isolation certification.

## Division of labour

| Role | Harness | Provider/model | Reasoning |
|---|---|---|---|
| Orchestrator | OpenCode | `opencode-go/deepseek-v4.1-flash` | `max` |
| Implementers | OpenCode | `opencode-go/deepseek-v4.1-flash` | `high` |
| Architecture/security reviewer | **Codex** | `codex/gpt-6-luna` | `xhigh` |
| Rust/testing/operability reviewer | **Codex** | `codex/gpt-6-luna` | `high` |
| Office/accessibility reviewer | **Codex** | `codex/gpt-6-luna` | `high` |
| Candidate self-test (optional) | **Codex** | `codex/gpt-6-luna` | `high` |

The implementers are not the reviewers. A reviewer runs in its own Codex
session, with its own context, and never receives the implementer’s hidden
reasoning. A Codex reviewer must not be confused with an OpenCode subagent that
merely uses a GPT-6 Luna model ID.

## Workspace

Create an OpenCode workspace pointing at the exact integration worktree, for
example:

```text
title: Quality Harness Wave 1 — Codex Luna review
path:  /home/james/Escritorio/Development/sf-qh/integration
```

The reviewer must record the candidate SHA and confirm `HEAD` matches before
reading. The workspace is disposable; do not reuse it for implementation.

## Reviewer prompts

Every reviewer receives:

- candidate SHA and base SHA;
- exact `git show --stat` / `git diff` scope;
- the implementer evidence summary;
- the role focus;
- the mandatory output schema.

Example role focus:

- **Architecture/security:** second authority paths, run-dir ownership, resume
  binding, exact volume set, provider/GitHub fail-closed behavior, MCP runtime
  boundary, CI gate weakening.
- **Rust/testing/operability:** stage/state semantics, latest-stage evidence,
  cleanup result, bounded subprocesses, cassette replay/redaction, manifest
  candidate/dirty truthfulness, test adequacy.
- **Office/accessibility:** promise-handling behavior, scoped lint suppressions,
  keyboard focus, busy/accessible name, axe automated floor, fake Core state,
  i18n/responsive checks, dead-code visibility.

Output exactly:

```text
verdict: APPROVE | CHANGES_REQUESTED | NOT_TESTED
inspected: [...]
findings:
  - id: QH-001
    severity: BLOCKER | REQUIRED | OPTIONAL
    file: path:line
    evidence:
    impact:
    correction:
    verification:
not_tested: [...]
```

A finding without file/line/evidence is not a finding. A reviewer does not edit,
commit, push, merge or accept work.

## Read-only limitation

Codex `auto` mode is workspace-write and is **not** a read-only security
boundary. The reviewer prompt forbids edits, but the hard boundary is the
disposable workspace plus the orchestrator’s review of the diff. For true
read-only verification, export the exact git object and run tests under a
read-only/containerized environment; the current `scripts/ci/verify-candidate.sh`
is a **self-test**, not independent acceptance.

## Evidence

The orchestrator records the reviewer agent ID/session, model, reasoning level,
candidate SHA, verdict and findings. A Codex review is advisory evidence for the
development change; the Factory’s independent Verifier, reviewers and Quality
Gate remain authoritative for product acceptance.
