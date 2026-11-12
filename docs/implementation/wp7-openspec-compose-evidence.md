# WP7 OpenSpec Compose validation evidence

**Status:** implemented and locally exercised on 2026-11-09; **not** independent
acceptance, certification or an isolation claim. Core remains authoritative.

- Branch: `factory/quality-wp7-openspec-compose`
- Baseline: `bcb8b5b`
- Implementation commit: `1b23d50` (includes `85c2bae`, `0377ced`,
  `06d54e2`, `73bfb76`; this docs commit records that exact code candidate; the
  code is unchanged by the docs commit).
- Scope: an operator-authenticated Core read projection, a profile-gated
  Compose validation job using the pinned OpenSpec 1.16.1 image, a bounded
  container-side validation script, deterministic contract tests and static
  Compose policy coverage.

## What changed

| Area | Files |
|---|---|
| Core read model | `native/src/openspec.rs` |
| Core endpoints | `native/src/bin/core.rs` (`GET /agentic/requests/{id}/openspec`, `POST /agentic/requests/{id}/openspec/archive`) |
| Archive persistence | `native/src/db/planning.rs` |
| Validation script | `deploy/openspec/validate.mjs` |
| Pinned image | `deploy/openspec/Dockerfile` (OpenSpec 1.16.1, Node 22.23.2) |
| Compose job | `compose.yaml` (`openspec-validate`, profile `openspec-validation`) |
| Token volume | `native/src/bin/compose_init.rs`, `compose.yaml` (`factory-config-openspec`) |
| Contract tests | `native/tests/openspec_compose_validation.rs`, `deploy/openspec/validate.test.mjs`, `deploy/openspec/fixtures/openspec-read-model.json` |
| Compose policy | `.github/scripts/check-compose-policy.py`, `.github/scripts/test_check_compose_policy.py`, `.github/workflows/native-rust.yml` |
| Docs | `docs/operations/compose-deployment.md`, this document |

This revision also repairs the independent review findings: Core now has an
idempotent archive transition with stored-artifact/projection drift checks; the
Compose policy enforces an exact single volume allowlist; the read path
recomputes the stored artifact JSON SHA-256; Node tests cover bare/escaped and
mismatched Core responses; and the Core fetch deadline now covers body
consumption.

The endpoint returns the latest durable projection only when the stored
artifact validates, its change id/revision match the projection row, and all
manifest hashes match. It is authenticated with the Core operator token, bounds
the request id, and returns `OPENSPEC_NOT_FOUND`, `OPENSPEC_ARTIFACT_MISSING`,
`OPENSPEC_ARTIFACT_INVALID` or `OPENSPEC_PROJECTION_STALE` as typed failures.
It never returns the operator token, master key or unrelated Core secrets.

The validation job bounds the Core response to 8 MiB, each rendered file to
512 KiB, total artifacts to 2 MiB, CLI output to 256 KiB and the whole fetch
(including body consumption) to 15 seconds by default. The fetch abort deadline
stays active until the body is fully read.

The archive endpoint recomputes the exact stored artifact JSON SHA-256 before
decoding it, then verifies the change id, revision and every projection/manifest
hash. Under the same write lock and `BEGIN IMMEDIATE` transaction it also
requires the target revision to remain the latest for the request. It archives
the durable projection only when the binding matches. A replay returns
`alreadyArchived: true` without changing `updated_at`; a changed artifact, stale
binding, superseded revision or different archived identity is rejected as
drift/immutability. Archiving never accepts a candidate, closes a finding or
grants delivery authority.

## Verification results

From `native/`:

```text
cargo fmt --all -- --check
PASS

cargo test -p factory-native --lib
737 passed; 0 failed; 2 ignored

cargo test -p factory-native --test openspec_compose_validation
5 passed; 0 failed

cargo test -p factory-native --test planning_pipeline
1 passed; 0 failed

cargo test -p factory-native --bin factory-core openspec_projection_route
1 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

From the repository root:

```text
node --test deploy/openspec/validate.test.mjs
8 passed; 0 failed

podman compose -f compose.yaml config
PASS (podman-compose 1.6.0)

podman compose --profile openspec-validation -f compose.yaml config
PASS

docker-compose -f compose.yaml config --format json \
  | python3 .github/scripts/check-compose-policy.py
compose-policy: PASS

docker-compose --profile openspec-validation -f compose.yaml config --format json \
  | python3 .github/scripts/check-compose-policy.py
compose-policy: PASS

COMPOSE_CONFIG_JSON=<base or profile JSON> \
  python3 .github/scripts/test_check_compose_policy.py
32 tests; OK (profile-specific negative tests skip on the base config)
```

Host Node was used only to run the deterministic Node contract test. The
installed validation job runs the same script inside the pinned image; it does
not use host Node or a host OpenSpec installation.

## Pinned image

```text
podman compose --profile openspec-validation -f compose.yaml build openspec-validate
openspec --version -> 1.16.1
image: localhost/software-factory-openspec:1.16.1
image ID: dfb131b984ad267a4fb82ff4beffc1dcb07dab62703264110003553456ac77ac
local digest: sha256:6881e126148eca823c05258ab7edd6b8d1c3269319b3eaa1666f06504e453c51
```

The base image was moved from `node:22.12.0` to `node:22.23.2` after the first
build reported `EBADENGINE` warnings from OpenSpec's dependencies. The rebuild
with 22.23.2 completed without those warnings and still reported OpenSpec
`1.16.1`. The image and the job environment set `OPENSPEC_NO_UPDATE_CHECK=1`,
`OPENSPEC_TELEMETRY=0` and `NO_COLOR=1`. The base image is tag-pinned, not
digest-pinned.

## Real Compose run

A real `podman compose` run was executed in an isolated Compose project
(`wp7-openspec`). Because no paid model inference is authorized, the Core
endpoint was served by a temporary HTTP fixture container on the private control
network. The fixture is `deploy/openspec/fixtures/openspec-read-model.json`,
which `native/tests/openspec_compose_validation.rs` proves is byte-for-byte the
Core renderer/read-model output. The job container, image, entrypoint, token
volume, control network, tmpfs workspace and pinned OpenSpec CLI were the
product definitions; only `FACTORY_CORE_URL` pointed at the fixture service.

```sh
FACTORY_OPENSPEC_REQUEST_ID=wp7-compose-fixture \
  podman compose -p wp7-openspec \
    -f compose.yaml -f /tmp/opencode/wp7-openspec-run/compose.override.yaml \
    --profile openspec-validation run --rm --no-deps openspec-validate
```

Exact result (exit 0):

```json
{"schemaVersion":1,"status":"PASS","requestId":"wp7-compose-fixture","changeId":"add-bounded-request-validation","changePath":"openspec/changes/add-bounded-request-validation","revision":1,"validated":true,"archived":false,"openspecVersion":"1.16.1","versionOutput":"1.16.1","artifactContentSha256":"4bc709ab27f04ad60b4e5b5d11c8257a72af6ddf95fa55f95ee4e5bd8a6f615a","artifactHashes":[{"path":"proposal.md","sha256":"5c118fe9db6a51e11338cb13e5443319ac69da31c01dcc2b747d5e8e24632844"},{"path":"specs/add-bounded-request-validation/spec.md","sha256":"a6472d50639d0547b009622e9addb45ff981c2e46844b4539921661dea1fa5c1"},{"path":"design.md","sha256":"a4be83659a25bb74aa327604656bb50dc29ed8b9d3b2a9805806385545ca689b"},{"path":"tasks.md","sha256":"7aa3344072dcf13dff62b99a323e319f859718ff83173677b322d69e31ea40b1"}],"command":"openspec validate add-bounded-request-validation --strict","exitCode":0,"checks":{"coreResponse":true,"versionPinned":true,"artifactHashesMatch":true,"openspecValidate":true,"workspaceCleaned":true},"validation":{"stdout":"Change 'add-bounded-request-validation' is valid","stderr":""},"error":null,"code":null}
```

The first real run failed closed with `OPENSPEC_VALIDATE_FAILED` because the
pinned strict validator reported:

```text
[WARNING] .../spec.md: Text above the first "## " section is not carried into
the main spec by archive.
```

The renderer was repaired rather than weakening the check: `specs/<capability>/spec.md`
now starts at `## ADDED Requirements`, and the change identity/revision remain
in `proposal.md`, `design.md` and the manifest. Artifact validation now rejects
a spec preamble, the fixture hashes were regenerated, and the rerun passed.

Cleanup after the run: the `wp7-openspec` containers, networks and named volumes
were removed; the committed fixture and result JSON remain as evidence. The
later stale-latest archive repair changes only the Core archive transaction, not
the validation image or script, so this Compose run was not rerun for it.

## Compose-init token volume

`factory-config-openspec` contains only `operator.token` and `.ready`, not the
master key or the other role credentials. The compiled
`factory-compose-init` binary was run in a container against temporary
directories and produced:

```text
Factory role configuration material is ready; no secrets were printed.
TOKEN_COPY_MATCH
640 10001:10001 config-openspec/operator.token
640 10001:10001 config-openspec/.ready
```

The real Compose run used the init-produced operator token copied into that
volume, because the locally prebuilt `software-factory-core:dev` image predates
this `compose-init` change. A normal `podman compose up --build` rebuilds init
from this source.

## Negative and authority evidence

- `native/src/openspec.rs` tests reject stale revisions, mismatched hashes,
  tampered artifacts, invalid artifact hashes, changed stored JSON bytes, an
  unvalidated archive and unsafe request ids.
- `native/src/db/planning.rs` tests prove the archive transition is idempotent,
  preserves `updated_at` on replay, rejects a changed hash/revision/identity,
  requires the revision to remain latest under the write transaction, and
  leaves the archived row immutable.
- The Core route test rejects unauthenticated calls, a missing request and a
  201-character request id for both the read and archive routes on both router
  variants.
- `deploy/openspec/validate.test.mjs` rejects a file/hash mismatch, bare or
  escaped non-object responses, a mismatched request identity, an unpinned
  version, a Core 401 (without leaking the token), a stalled body deadline and a
  failing `openspec validate`, always returning non-zero and cleaning the
  workspace. The fail-closed cases prove the fake OpenSpec executable was never
  invoked.
- The Compose policy rejects a writable job root, extra capabilities, an egress
  network, any volume other than the exact `factory-config-openspec` mount
  (including `factory-projects`/`factory-data`), a writable config volume and
  an environment-supplied token.
- A PASS never accepts a candidate, closes a finding or edits the Core database
  or project source. The job has no project volume, no Core data volume, no
  egress network and no published port.

## Not tested / limitations

- `NOT_TESTED`: a real Core database/API request whose projection was produced
  by the paid model planning pipeline. The endpoint and read model are covered
  by Rust contracts, and the real Compose run used the Core-rendered fixture via
  a temporary Core HTTP fixture.
- `NOT_TESTED`: project working-tree file drift (an edited
  `openspec/changes/<id>` copy on disk) is not read by Core. Drift detection
  covers the stored artifact JSON versus the durable projection/manifest hashes
  and the archive binding; repository-file drift remains a future projection
  check.
- `NOT_TESTED`: the Docker Compose provider path and Windows. The run above used
  rootless Podman with `podman-compose` 1.6.0.
- `NOT_TESTED`: base-image digest pinning. The image is tag-pinned.
- `NOT_APPLICABLE` for browser/Office acceptance: this is an internal one-shot
  validation job and read-only projection. It adds no Office route, control or
  user action; the existing Director plan-review surface is unchanged. Office
  impact is `none` with that rationale.
- No paid model inference, candidate acceptance, finding closure or delivery
  effect was exercised or claimed.
