# Compose unified-intake lifecycle — CI evidence (2026-10-28)

**Status:** author-dispatched, candidate-bound GitHub Actions evidence on the
supported rootless Podman Compose runner. The model and coding provider are
deterministic fixtures. This is **not** live provider inference, not Office
browser acceptance on Compose and not independent acceptance.

## Candidate identity

| Item | Value |
|---|---|
| Factory implementation candidate | `056c50248e0b74a84be2070e6e148df5348c9e36` |
| Branch | `campaign/office-ux-compose-intake-20261028` |
| Base | `campaign/office-ux-wave2-integration-20261028` at `e3afcb6` |
| Workflow | Compose end-to-end (manual), run `40742928074` |
| Run URL | https://github.com/JamesMorales04/software-factory/actions/runs/40742928074 |
| Result | `success` (completed, 10m0s job) |
| Runtime | GitHub-hosted rootless Podman + `/usr/bin/podman-compose` |
| Execution command | `bash scripts/compose-e2e.sh` |
| Project isolation | explicit `-p software-factory-e2e`; volumes/containers used the dedicated E2E project |

## Exact intake identities

| Item | Value |
|---|---|
| Intake request | `bfafdc1b-8d2a-4e7c-833a-dfa8b761d72a` |
| Campaign | `7464a25e-ecae-4688-85da-8d003c8f58e2` |
| Frozen plan hash | `a702c4c55c3bbb5e0d6b520fed64e17e34e2501b2c3ad64d00d46c50dac6f9af` |
| Exact profile revision | `d430dba3-8653-4f30-a109-2e1c5b13a1e9` |
| Lead Attempt | `b9bf43ae-61a8-4745-ac1f-e634d87ae560` |
| Candidate | `be2c44cd-eb84-4089-b235-b0d0603231ec` |
| Candidate commit | `777de0cb5df42219686dba6574500e417468916b` |
| Verifier receipt | `35c555bd-a904-4469-9151-30d87709b409` |
| Reviewer receipt | `a349b5df-d75b-4b5c-8c69-82b8468fbd6f` |
| Gate | `ACCEPTED` |
| Reviewer mechanism | `MODEL` through the deterministic Compose model fixture |
| Restart persistence | request, Campaign, candidate, receipts and Gate preserved |

The legacy Compose journey ran in the same stack and remained green:

| Item | Value |
|---|---|
| Legacy candidate | `ebe7d298-8e19-481c-91fe-3eeca0e3d9f2` |
| Legacy Attempt | `6159bd7f-9f8c-420b-be23-7f547520616f` |
| Legacy Gate | `ACCEPTED` |

CI summary line:

```json
{"status": "PASS", "legacy_candidate_id": "ebe7d298-8e19-481c-91fe-3eeca0e3d9f2", "legacy_attempt_id": "6159bd7f-9f8c-420b-be23-7f547520616f", "intake_request_id": "bfafdc1b-8d2a-4e7c-833a-dfa8b761d72a", "intake_campaign_id": "7464a25e-ecae-4688-85da-8d003c8f58e2", "intake_attempt_id": "b9bf43ae-61a8-4745-ac1f-e634d87ae560", "intake_candidate_id": "be2c44cd-eb84-4089-b235-b0d0603231ec", "intake_candidate_commit": "777de0cb5df42219686dba6574500e417468916b", "intake_plan_hash": "a702c4c55c3bbb5e0d6b520fed64e17e34e2501b2c3ad64d00d46c50dac6f9af", "intake_profile_revision_id": "d430dba3-8653-4f30-a109-2e1c5b13a1e9", "intake_verifier_receipt_id": "35c555bd-a904-4469-9151-30d87709b409", "intake_reviewer_receipt_id": "a349b5df-d75b-4b5c-8c69-82b8468fbd6f", "gate": "ACCEPTED", "model_mechanism": "DETERMINISTIC_FIXTURE", "restart_persistence": true, "paid_inference": false}
```

## What the Compose run exercised

- The supported `podman compose -p software-factory-e2e -f compose.yaml -f
  compose.e2e.yaml` stack: init, Core/Office static build, Supervisor, Worker,
  nested runtime engine and broker, verifier, reviewer and disposable Git
  fixture.
- A new E2E-only `model-fixture` service on the internal factory-control
  network. It serves deterministic OpenAI-compatible
  `/v1/chat/completions` for Lead assessment and semantic review from a
  read-only `/context` volume, reports measured usage and receives no operator,
  GitHub or provider credential.
- Core reached the fixture through the new test-only
  `FACTORY_ALLOW_TEST_UPSTREAM=1` +
  `FACTORY_TEST_UPSTREAM_HOSTS=model-fixture` policy. Provider settings and
  model-gateway validation both enforce the exact host; production defaults
  remain HTTPS-only.
- The shell configured the deterministic provider route, an executable
  execution profile for Lead/Implementer/Semantic Reviewer, the explicit
  non-autonomous execution policy, and submitted an intake request.
- Assessment and freeze used the fixture model; approval used the exact plan
  hash through the operator boundary and created the Campaign and Lead Attempt.
- The running Worker claimed the Lead Attempt, produced the exact Git candidate
  and the deterministic provider fixture candidate `src/provider.txt`. The real
  verifier and reviewer containers produced independent receipts; the reviewer
  recorded `MODEL` provenance and measured usage. The deterministic Quality Gate
  recorded `ACCEPTED`.
- API assertions covered request `COMPLETED` + `gateOutcome`, Campaign/task/
  candidate lineage, changed path, verifier/reviewer receipts and Gate.
- `compose down` followed by `compose up -d` preserved the intake request,
  Campaign, candidate commit, receipts and Gate alongside the legacy journey.

## Commands run around the CI result

```bash
cargo fmt --all -- --check
cargo clippy -p factory-native --all-targets --locked -- -D warnings
cargo test -p factory-native --lib
cargo test -p factory-native --lib provider_catalog::tests
cargo test -p factory-native --lib model_gateway::tests
bash -n scripts/compose-e2e.sh
python3 -m py_compile scripts/fixtures/compose-model-fixture.py
podman compose ... config
cargo xtask e2e intake-full
cargo xtask e2e office
gh workflow run compose-e2e.yml --ref campaign/office-ux-compose-intake-20261028
```

Results:

- Format, Clippy, `bash -n`, Python compilation and Compose config parsing:
  clean.
- `factory-native` library: **338 passed, 2 ignored, 0 failed**.
- Provider-catalog allowlist tests: 3 passed, including acceptance of the
  configured `model-fixture` host and rejection of unlisted hosts, plain HTTP
  without the flag, and plain HTTP without the host entry.
- Model-gateway tests: 3 passed, including the same allowlist boundaries.
- Host `cargo xtask e2e intake-full`: PASS with candidate
  `46a1fce49d6f2e91eb000d97c89d3e89d7fc5b2f` and Gate `ACCEPTED`.
- Host `cargo xtask e2e office`: PASS with candidate
  `3a081920318b11908a248a347e56c9331560f817` and Gate `ACCEPTED`.
- Compose workflow run: **success** at implementation SHA `056c502…`.

## Deterministic vs live

- **Deterministic:** Lead assessment, semantic review, provider/model settings
  rates and the coding provider are fixtures. The workflow summary reports
  `model_mechanism: DETERMINISTIC_FIXTURE` and `paid_inference: false`.
- **Live inference:** `NOT_TESTED`. No credential or paid budget envelope was
  used. This evidence says nothing about real model quality or real OpenCode/
  Codex inference.
- **Office/Chromium on Compose:** `NOT_TESTED`. The Compose workflow verified
  Core API projections. The Office browser journey remains host/lane evidence,
  not a Compose browser result.
- **Independent acceptance:** `NOT_TESTED`. The author dispatched the workflow;
  no independent verifier/reviewer or Product Owner accepted this candidate.
- **Delivery:** no branch/PR/check/merge effect was exercised beyond the
  disposable fixture repository.

## Explicit limitations

- The deterministic model fixture is E2E-only and is not part of the installed
  product. Its plain-HTTP host allowance is bounded by the existing test flag
  and an exact host list; it is not a production transport policy.
- The intake plan has one implementation WorkUnit. Parallel WorkGraph/fan-in,
  repair cycles, provider cancellation and real provider sessions remain
  unexercised.
- The Compose workflow runs on GitHub-hosted rootless Podman. This evidence is
  for that runner and exact SHA only; it does not certify another host.
- The run is author-dispatched CI, not independent acceptance or a release
  decision.

## What this evidence supports

It supports a truthful claim that the supported rootless Compose acceptance now
exercises the unified intake lifecycle end to end from request/policy through
assessment, freeze, explicit operator approval, real Supervisor/Worker/verifier/
reviewer containers, deterministic Gate `ACCEPTED`, exact projections and
restart persistence on the real Compose stack. It does not support a claim of
live provider inference, Compose browser acceptance, isolation certification or
independent acceptance.
