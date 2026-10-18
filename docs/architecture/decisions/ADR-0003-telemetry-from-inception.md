---
id: ADR-0003
status: Accepted
date: 2026-10-18
owner: product-owner
---
# ADR-0003: Telemetry from the first functional slice

## Context
The Factory must diagnose failures, recover Runs, track actual resource usage, observe agent sessions and evaluate routing. Adding observability only after core integration would hide early failures.

## Alternatives
1. Introduce telemetry at the end of F1 — easier startup, poor early forensics.
2. Minimal instrumentation from the first Core slice — some upfront cost, reliable evidence from the outset.

## Decision
Choose option 2: transactionally durable business events for authoritative history, structured redacted logs, OpenTelemetry traces/metrics and local-first export. OTLP remains optional. Exporter failure cannot change a business transition.

## Consequences
Shared identities and redaction policies are required from day one. Telemetry is diagnostic, not authorization or proof that model claims are true.

## Verification
Creation/recovery of an Action emits correlated events/traces; exporter downtime leaves state safe; secret canaries are absent from logs and spans.
