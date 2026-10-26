# Native Rust developer-preview testing guide

**Status:** tested local Linux developer-preview workflow. This is **not**
production deployment, SEC-01 isolation certification or live paid model
certification. TRUSTED_LOCAL processes have broad host access.

The authoritative status remains the [migration closure ledger](native-rust-closure.md).
The one-command disposable UAT is
[`scripts/developer-preview-uat.sh`](../../scripts/developer-preview-uat.sh).

## 1. Supported environment

- Linux with rootless Podman 6.x.
- Rust 1.90.0 (pinned by `rust-toolchain.toml`).
- Node 22.23.2 and npm for the Office/browser tooling.
- For real-Chromium checks: `playwright-core` 1.64.1 and a Chromium cache
  installed by the pinned Playwright CLI.
- No paid provider or model calls are needed for this guide.

## 2. Build and static gates

```sh
git clone https://github.com/JamesMorales04/software-factory.git
cd software-factory
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.90.0 test --workspace --locked
cargo +1.90.0 build --locked --bins
cargo +1.90.0 audit --file Cargo.lock

cd office && npm ci && npm run lint && npm run typecheck && npm run build && cd ..
cd office/e2e-browser && npm ci && cd ..
```

Install the pinned Chromium for deterministic browser checks:

```sh
cd office/e2e-browser
npx --yes playwright@1.64.1 install --with-deps chromium
cd ../..
```

Build the runtime image used by `factory start`:

```sh
podman build --network=host -t localhost/software-factory-core:dev \
  -f deploy/Containerfile.core .
```

## 3. One-command disposable UAT

The script creates a disposable Git project and data volume, starts the real
Core container plus host Supervisor/Worker, runs a deterministic fixture
Campaign through independent verification and review to `ACCEPTED`, restarts the
installation, verifies persistence, and cleans up.

```sh
./scripts/developer-preview-uat.sh
```

Evidence is written to `./developer-preview-evidence.json` and a sibling
`developer-preview-artifacts-<timestamp>/` directory by default. Set
`FACTORY_UAT_EVIDENCE=/absolute/path.json` to choose another location.
`FACTORY_UAT_KEEP=1` keeps the temporary repository/logs for debugging.

The script exits non-zero if any candidate, verifier, reviewer, gate,
persistence or cleanup assertion fails. It does not use a paid model.

## 4. Manual lifecycle

Set unique names and loopback endpoints:

```sh
export FACTORY_CORE_URL=http://127.0.0.1:8877
export FACTORY_IMAGE=localhost/software-factory-core:dev
export FACTORY_CONTAINER_NAME=factory-preview
export FACTORY_VOLUME=factory-preview-data
export FACTORY_ENV_FILE=/tmp/factory-preview.env
export FACTORY_PROJECT_ROOTS=/tmp/factory-projects
export FACTORY_SUPERVISOR_URL=http://127.0.0.1:8878
export FACTORY_HOST_EXECUTION=1
export FACTORY_BIN_DIR=$PWD/target/debug
mkdir -p "$FACTORY_PROJECT_ROOTS"
```

Create a disposable Project repository:

```sh
mkdir -p "$FACTORY_PROJECT_ROOTS/demo"
git -C "$FACTORY_PROJECT_ROOTS/demo" init -q
git -C "$FACTORY_PROJECT_ROOTS/demo" config user.name "Factory Preview"
git -C "$FACTORY_PROJECT_ROOTS/demo" config user.email "preview@example.invalid"
printf 'seed\n' > "$FACTORY_PROJECT_ROOTS/demo/seed.txt"
git -C "$FACTORY_PROJECT_ROOTS/demo" add seed.txt
git -C "$FACTORY_PROJECT_ROOTS/demo" commit -qm seed
```

Start and inspect:

```sh
target/debug/factory start
set -a; . "$FACTORY_ENV_FILE"; set +a
target/debug/factory doctor
target/debug/factory status
curl -fsS "$FACTORY_CORE_URL/health"
```

Open `http://127.0.0.1:8877/` in a browser and unlock the Office with
`FACTORY_OPERATOR_TOKEN`. Register the Project, configure its allowed paths and
quality profile, create an Action with functional and negative checks, create a
Campaign, authorize R0/FIXTURE execution, then create a Run and Attempt.

The host Worker claims the Attempt, creates a candidate, and releases it. Run
the independent verifier and deterministic reviewer:

```sh
FACTORY_CORE_URL="$FACTORY_CORE_URL" \
FACTORY_VERIFIER_TOKEN="$FACTORY_VERIFIER_TOKEN" \
FACTORY_CANDIDATE_ID="<candidate-id>" \
FACTORY_VERIFIER_PRINCIPAL=preview-verifier \
FACTORY_VERIFIER_WORKSPACE=/tmp/factory-preview-verifier \
FACTORY_VERIFIER_IMAGE=docker.io/library/alpine:3.20 \
FACTORY_VERIFIER_TEST_BACKEND=local \
FACTORY_VERIFIER_TEST_VOLUME_ROOT=/tmp/factory-preview-verifier-volumes \
target/debug/factory-verifier

FACTORY_CORE_URL="$FACTORY_CORE_URL" \
FACTORY_REVIEWER_TOKEN="$FACTORY_REVIEWER_TOKEN" \
FACTORY_CANDIDATE_ID="<candidate-id>" \
FACTORY_REVIEWER_PRINCIPAL=preview-reviewer \
target/debug/factory-reviewer
```

Inspect evidence and budgets:

```sh
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" \
  "$FACTORY_CORE_URL/quality/candidates/<candidate-id>/receipts"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" \
  "$FACTORY_CORE_URL/quality/candidates/<candidate-id>/gate"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" \
  "$FACTORY_CORE_URL/model/calls?limit=100"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" \
  "$FACTORY_CORE_URL/observability"
```

Stop, restart and verify persistence:

```sh
target/debug/factory stop
target/debug/factory start
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" \
  "$FACTORY_CORE_URL/projects"
target/debug/factory stop
podman volume rm "$FACTORY_VOLUME"   # optional destructive test cleanup
```

`factory stop` removes the container but keeps the data volume by design.
`--purge` requires interactive confirmation; removing the test volume directly
is suitable for a disposable preview.

## 5. Semantic Reviewer and model-call budgets

The deterministic reviewer is the default and needs no model. To exercise the
model path against a deterministic fake OpenAI-compatible upstream, create a
private Core-only model env file and start Core with it:

```sh
install -m 600 /dev/null /tmp/factory-preview-model.env
cat > /tmp/factory-preview-model.env <<'EOF'
FACTORY_MODEL_URL=http://127.0.0.1:9000/v1
FACTORY_MODEL_TOKEN=deterministic-test-token-1234
FACTORY_MODEL_PROVIDER=opencode-go
FACTORY_MODEL_NAME=deepseek-v4.1-flash
FACTORY_ALLOW_TEST_UPSTREAM=1
EOF
export FACTORY_CORE_MODEL_ENV_FILE=/tmp/factory-preview-model.env
target/debug/factory doctor
target/debug/factory start
```

`factory start` passes only the allowlisted Core model keys to the Core
container as an additional `--env-file`. If a running container was created
with a different model env fingerprint, it is recreated instead of silently
reused. Do not commit the file or place provider credentials in the shell
environment. Then run the reviewer with `FACTORY_REVIEW_MECHANISM=MODEL`. A
model approval is corroborating only: the deterministic pre-review must also
approve before the reviewer can submit `PASS`. Malformed output produces a
durable `FAIL` and the gate becomes `REPAIR_REQUIRED`, never `ACCEPTED`.

The Lead uses the same Core-owned gateway. `FACTORY_LEAD_FIXTURE_ASSESSOR=1`
exercises the typed planning path without a model. Real provider credentials
must never be committed; use a separately authorized environment. No paid calls
are part of this guide.

## 6. CAP-014 raw-output collection

With an authorized Core gateway:

```sh
FACTORY_OPERATOR_TOKEN=... target/debug/factory-s4d-evaluate execute \
  http://127.0.0.1:8765 cap014-preview /tmp/cap014-preview.json
```

This records raw fixture outputs and settled model-call usage. It does **not**
score them. Independent blinded score sheets and `compare` remain required for
any semantic-quality claim. A denied budget writes `FAILED` evidence and exits
non-zero.

## 7. Factory-managed browser check

```sh
FACTORY_PLAYWRIGHT_CORE=$PWD/office/e2e-browser/node_modules/playwright-core \
PLAYWRIGHT_BROWSERS_PATH=$HOME/.cache/ms-playwright \
target/debug/factory-browser run \
  --cwd /tmp/factory-browser-app \
  --log-dir /tmp/factory-browser-logs \
  --endpoint http://127.0.0.1:4173 \
  --ready-path /health \
  --selector '#app-ready' \
  --evidence /tmp/factory-browser-logs/evidence.json \
  -- npm start
```

Missing tooling exits `3` (`NOT_TESTED`), never `PASS`. The manager verifies
port closure and process-group absence after the check. Managed-server health is
infrastructure evidence, not independent browser acceptance.

## 8. Stop and cleanup rules

- `factory stop` stops host Supervisor/Worker and removes the Core container;
  the data volume is retained.
- Remove a disposable volume only after stopping and confirming it is the test
  volume.
- Remove temporary projects, verifier workspaces, browser profiles and evidence
  directories.
- Check for leftovers:

```sh
pgrep -af 'factory-supervisor|factory-worker'
podman ps -a --format '{{.Names}} {{.Status}}'
podman volume ls --format '{{.Name}}'
```

## 9. What this does not prove

- No SEC-01 ISOLATED certification or credential confinement.
- No live paid provider coding or model-quality certification.
- No Windows, production deployment, GitHub delivery or human security review.
- A TRUSTED_LOCAL test server can still ignore `HOST`/`PORT`; browser cleanup is
  control-plane policy, not OS isolation.
- The deterministic mock upstream demonstrates integration, not live model
  quality.

For the full acceptance matrix and residual gaps see the
[native Rust closure ledger](native-rust-closure.md) and
[final closure evidence](../evidence/native-rust-final-closure-2026-10-25.json).

## 10. Live provider UAT (separately authorized, paid)

This guide's deterministic path does not use a paid model and is not evidence
that live coding works. A separately authorized live UAT used the real
OpenCode `1.18.32` provider server, `opencode-go/deepseek-v4.1-flash`, the
sandboxed Worker tool loop, an independent verifier and a real semantic
Reviewer. The journey reached a candidate `ACCEPTED` gate and the measured
evidence is in [F1-S4-LIVE-UAT](../evidence/f1-s4-live-uat-2026-10-25.md).
The live run also exposed integration defects (provider egress/session
headers, sandbox paths, SSE streaming, lease heartbeat and finalize resilience)
that are repaired only in the accompanying repair PR; the merged baseline does
not contain them. Do not treat this guide or the deterministic fixture as a
substitute for the live report.

