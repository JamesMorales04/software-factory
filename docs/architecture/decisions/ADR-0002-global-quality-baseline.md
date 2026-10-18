---
id: ADR-0002
status: Accepted
date: 2026-10-18
owner: product-owner
---
# ADR-0002: Mandatory global quality baseline

## Context
The Factory serves different languages and products. We need universal guarantees without blindly running every tool in every project.

## Alternatives
1. One fixed tool suite for all projects — unnecessary cost and false failures.
2. Only project-specific profiles — allows shared safety guarantees to erode.
3. Global acceptance guarantees with additive project profiles.

## Decision
Choose option 3. Candidate/spec/policy identity, isolation, permitted scope, functional correctness, relevant regression, security, independent verification, evidence provenance and exact delivery identity are mandatory. Project profiles add checks; they cannot weaken the baseline.

## Consequences
Concrete tools vary with stack and risk. `SKIPPED`/`NOT_TESTED` never mean `PASS`. The Core owns risk and acceptance; agent judgment is advisory.

## Verification
F1 must show that a missing/failing required check blocks acceptance, and that results are tied to the exact candidate SHA.
