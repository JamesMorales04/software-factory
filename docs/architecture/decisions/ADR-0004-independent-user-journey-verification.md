# ADR-0004: Independent black-box user journey verification

- **Status:** Accepted (product-owner decision)
- **Context:** Tests written and executed only by an implementer may inadvertently validate its own assumptions or rely on files from its workspace.
- **Decision:** An Implementer must run real product checks and repair defects, but final acceptance requires an **Independent Verifier** during review to exercise the exact frozen candidate commit in a clean environment, without editing candidate or acceptance oracles. Use CLI/API/Playwright/real Paseo paths appropriate to the feature. A separate Reviewer assesses code, security and evidence; the deterministic Quality Gate records the decision. For small changes, an independent Reviewer may fulfill the Verifier function if the read-only boundary remains effective.
- **Controls:** The author cannot modify independent oracles to pass. Identity, permissions, environment and evidence must be bound to the candidate. Repairs create a new candidate and trigger revalidation. Independence is about actual authority, process and evidence; using a different model alone is insufficient.
- **Consequences:** Some added verification expense; fewer false positives, more reliable UX/integration evidence and repeatable acceptance workflows.
