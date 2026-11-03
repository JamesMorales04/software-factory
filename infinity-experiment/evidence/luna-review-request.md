# Independent review request — Infinity-first Factory experiment

**Reviewer:** GPT-6 Luna via subscription-authenticated Codex
**Status:** **BLOCKED — NOT RUN.** No real provider inference is authorized in
this experiment. Do not run this packet until the Owner gives a separate,
specific, subscription-only authorization envelope.

## Scope

Review the experiment, not a production merge:

- Branch `experiment/infinity-runtime-20261102`
- Base `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e`
- Infinity pin `1abc6fcff1f186124f924b56aa92c3a3684c90c5`
- Decision: `docs/architecture/decisions/ADR-0030-infinity-runtime-experiment.md`
- Implementation: `infinity-experiment/`
- Evidence: `infinity-experiment/evidence/`

Do not approve a merge, production migration, paid inference, Extra Usage,
credits/balance fallback, Bedrock or an unregistered OAuth client.

## Reproduction (offline)

```sh
cd infinity-experiment
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
scripts/restart_evidence.sh evidence/restart-evidence.json
scripts/compare.sh evidence/comparison-$(date +%Y%m%d)
```

## Review questions

1. **Correctness:** Does the SQLx store faithfully implement Infinity’s
   `ConversationStore`/`StateStore` ordering, dedup, ancestor and compaction
   semantics? Are `BEGIN IMMEDIATE`, unique keys and ordinal bounds correct?
2. **Durability:** Is the effect state machine safe against every crash point?
   Does marker verification correctly distinguish “effect applied but callback
   missing” from “outcome unknown”? Can any path replay an effect or deliver a
   stale callback?
3. **Concurrency:** Are callback claim/delivery, append and spawn atomic across
   processes? Are the 32-way callback and stale-callback tests sufficient?
4. **Recovery:** Does ambiguous-effect settlement avoid a zombie thread without
   inventing success or granting authority?
5. **Cancellation:** Does cancellation prevent unsettled effects and callbacks
   while preserving already-applied external effects?
6. **Security:** Is the fixed task allowlist / path validation / cleared env /
   timeout boundary honestly described as non-isolated? Are there injection or
   traversal paths?
7. **Provider feasibility:** Are the subscription matrix claims supported by
   the cited official docs? Is any claim fabricated, especially entitlement,
   no-overage, or “response metadata prevents a charge”?
8. **Comparison:** Is the measured comparison presented without overclaiming
   equivalence between the prototype and the full Factory governance pipeline?
9. **Recommendation:** Is “retain Factory, pilot selected Infinity components”
   supported, or does the evidence justify replacement/Infinity Code?
10. **Repo hygiene:** Any production-path, Wave 3, secret, credential or
    container-boundary violation?

## Required verdict

Return one of:

- `PASS — experiment evidence supports the recommendation`
- `PASS WITH CONDITIONS — list exact conditions`
- `FAIL — list blocking defects with file/line and reproduction`

Also state explicitly what remains `NOT_TESTED`, `BLOCKED` or `NOT_APPLICABLE`.

## Authorization template (Owner only)

```text
I authorize one subscription-only independent review by GPT-6 Luna through
ChatGPT-authenticated Codex, using the existing plan allowance only. API keys,
Extra Usage, credits, balance fallback, Bedrock and account switching remain
forbidden. Scope: read-only review of branch experiment/infinity-runtime-20261102
at commit <SHA>; no code execution outside the listed offline commands; no
merge.
```
