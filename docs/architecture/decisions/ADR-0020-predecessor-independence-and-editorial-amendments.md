# ADR-0020: Predecessor independence and editorial ADR amendments

- **Status:** Accepted
- **Date:** 2026-10-24
- **Supersedes:** editorial predecessor references in ADR-0001, ADR-0016, ADR-0018 and ADR-0019
- **Decision owners:** Product Owner migration-closure direction; implementation by the Lead Implementer

## Context

Software Factory is an independent product. Historical decisions referenced a
predecessor engineering harness by name, even where the decision itself did not
depend on that product. Leaving those names in active architecture makes the
repository look like a compatibility successor and keeps an unnecessary
external authority in the documentation graph.

Accepted ADRs must normally remain immutable. Removing obsolete terminology is
an editorial correction, not a change to the original decision. The repository
therefore needs a narrow, auditable amendment mechanism.

## Decision

1. Software Factory architecture, capabilities and operations are specified in
   Factory-owned terms. No active runtime, configuration, workflow, skill or
   architecture document requires a predecessor product.
2. An accepted ADR may be editorially amended only by adding an
   `Amended by: ADR-NNNN` line that points to an accepted amendment ADR.
   The amendment must state that the decision is unchanged and identify the
   editorial scope.
3. The original decision text and Git history remain available. Amendments must
   not fabricate prior approval, erase security findings or rewrite evidence.
4. The governance checker permits an otherwise immutable accepted ADR change
   only when the current file contains a valid `Amended by` reference to an
   existing accepted amendment ADR.
5. Product capabilities and engineering opportunities remain preserved in
   [capability decisions](../../product/capability-decisions.md); terminology
   cleanup must not delete a selected capability.

## Consequences

- The active tree can be predecessor-independent without falsifying history.
- Reviewers can distinguish an editorial amendment from a substantive policy
  change by the amendment ADR and the changed lines.
- A future substantive change still requires a new superseding ADR, not this
  mechanism.
