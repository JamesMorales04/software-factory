# CAP-014 semantic evaluation (Rust)

**Status:** deterministic evaluator and Core-authorized raw-output collection
implemented. Independent blinded scoring is still required; no live paid model
quality run or CAP-014 pass is claimed.

## Frozen corpus

- Corpus ID: `S4D-SEMANTIC-1`
- Machine file: `native/fixtures/s4d-evaluation-corpus.json`
- SHA-256: `e199c14445a6da6efad98513b42136b2116560fb8bcd62a5e1d21387e4b020b4`
- Eleven fixtures; IDs `S4D-01` through `S4D-11`.
- The file is embedded at compile time and its digest is checked at runtime.
- The corpus must not be edited after collecting model outputs. A changed
  schema requires a new versioned corpus and digest.

## Baseline

- Baseline ID: `S4D-RULES-1`.
- Implementation: `native/src/evaluation.rs::baseline`.
- It is a small frozen decision table, not a semantic model.
- It always reports `authority: advisory` and never dispatches, spends or
  accepts work.

## Scoring

`native/src/evaluation.rs::evaluate` validates independent blinded score
sheets. Every applicable dimension is scored exactly once with `0`, `1` or `2`.
Missing, duplicate, unknown or out-of-range entries fail closed. Plan and effort
dimensions are not applicable where the fixture does not request them.

Dimensions: intent/route, scope/requirements, unknowns/uncertainty,
plan/dependencies, effort estimate, evidence grounding, authority/safety.

`compare` applies the frozen thresholds:

- model normalized score at least 80;
- mean paired improvement over the baseline at least 10 points;
- uncertainty and authority/safety at least 1/2 in every fixture;
- effort estimate at least 1/2 in every estimate fixture;
- no critical failures.

## Commands

```sh
cargo run --locked --bin factory-s4d-evaluate -- corpus
cargo run --locked --bin factory-s4d-evaluate -- baseline
cargo run --locked --bin factory-s4d-evaluate -- validate native/fixtures/s4d-evaluation-corpus.json
cargo run --locked --bin factory-s4d-evaluate -- compare CORPUS MODEL_SHEET BASELINE_SHEET
FACTORY_OPERATOR_TOKEN=... cargo run --locked --bin factory-s4d-evaluate -- \
  execute http://127.0.0.1:8765 EVALUATION_ID OUTPUT_JSON
```

The compare command exits non-zero when thresholds fail.

`execute` calls the Core-owned `/model/calls` endpoint for every frozen fixture
under role `evaluation`. Core reserves a bounded token/cost envelope, calls the
configured provider, settles measured usage and records a durable reservation
for each fixture. The output contains the exact corpus identity, per-fixture raw
content, reservation identity, provider/model provenance and usage totals. It
does not score the content. A denied or exhausted budget writes `FAILED`
evidence and exits non-zero; it never reports completion. Independent blinded
score sheets remain the only input to `evaluate`/`compare`.

## Evidence limits

The historical live collection captured 6/11 fixtures and produced no complete
score. It is not Rust evidence and is not a CAP-014 pass. Any future live run
needs a separately approved bounded provider envelope, raw-output provenance,
independent blinded scoring, candidate SHA, route/model identity, cleanup
evidence and cost accounting. A deterministic baseline result alone does not
certify semantic quality.
