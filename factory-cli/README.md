# factory CLI

Host-native lifecycle CLI for the Software Factory Core container.

```sh
cargo build -p factory-cli --bin factory
cargo test -p factory-cli
cargo clippy -p factory-cli --all-targets -- -D warnings

factory doctor
factory start
factory status
factory logs
factory stop [--purge]
```

`start` validates Podman rootless and the local image, creates or reuses the
data volume, runs the container with a read-only root, dropped capabilities,
`no-new-privileges`, a loopback-only published port, no host network and no
Podman socket, then waits for `/health`. It generates six distinct secrets
(four role tokens plus the Attempt-grant and Supervisor-capability keys) into a
private `0600` env file if absent and never prints token values.
When `FACTORY_PROJECT_ROOTS` is set it also starts a private host Supervisor and
Worker, creates a separate Supervisor token file, and stops both on
`factory stop`.

`stop` keeps the data volume unless `--purge` is given and explicitly confirmed
by typing `purge` on a terminal. `logs` follows `podman logs`. `status` reports
container state, Core health and managed host services. The image serves the
built Rust Office and the printed URL is the loopback Office/Core origin.

## Configuration

| Variable | Default |
|---|---|
| `FACTORY_CORE_URL` | `http://127.0.0.1:8765` |
| `FACTORY_IMAGE` | `localhost/software-factory-core:dev` |
| `FACTORY_CONTAINER_NAME` | `factory-core` |
| `FACTORY_VOLUME` | `factory-core-data` |
| `FACTORY_ENV_FILE` | `$XDG_CONFIG_HOME/factory/core.env` or `$HOME/.config/factory/core.env` |
| `FACTORY_CORE_MODEL_ENV_FILE` | unset; optional Core-only model route env file |
| `FACTORY_PROVIDER_ENV_FILE` | unset; optional Worker-only provider route env file |
| `FACTORY_PROJECT_ROOTS` | unset; explicit comma-separated absolute Git roots |
| `FACTORY_SUPERVISOR_URL` | `http://127.0.0.1:8766` |
| `FACTORY_BIN_DIR` | directory of the running `factory` binary |
| `FACTORY_SUPERVISOR_IMAGES` | `docker.io/library/alpine:3.20` |

`FACTORY_CORE_URL` must be an HTTP origin on IPv4 loopback with an explicit
port. `start` publishes only `127.0.0.1:<port>`.

## Managed live route configuration

The six-key token env file is unchanged and remains the only source of Factory
role tokens. Live model and provider routes are opt-in through two separate
files; leaving either variable unset means no such route is configured.

Both files must be absolute, regular, non-symlink files with mode `0600`,
readable by the account running the CLI. They contain only `KEY=VALUE` lines and
blank/comment lines.
Unknown or duplicate keys, empty values, control characters and inline comments
after a value are rejected. Secret values belong only in these private files;
the CLI does not read model or provider credentials from the host environment
or command-line arguments. URLs containing user information, a query or a
fragment are rejected as inline-secret forms.

`FACTORY_CORE_MODEL_ENV_FILE` is passed to the Core container as an additional
`--env-file` and is never passed to the Worker or Supervisor. Its allowlist is:

- `FACTORY_MODEL_URL`, `FACTORY_MODEL_TOKEN`, `FACTORY_MODEL_PROVIDER`,
  `FACTORY_MODEL_NAME`, `FACTORY_MODEL_TIMEOUT_SECS`,
  `FACTORY_MODEL_INPUT_MICROS_PER_MILLION`,
  `FACTORY_MODEL_OUTPUT_MICROS_PER_MILLION`
- `FACTORY_MODEL_GLOBAL_MAX_INPUT_TOKENS`,
  `FACTORY_MODEL_GLOBAL_MAX_OUTPUT_TOKENS`,
  `FACTORY_MODEL_GLOBAL_MAX_COST_MICROS`
- `FACTORY_LEAD_MODEL_MAX_INPUT_TOKENS`,
  `FACTORY_LEAD_MODEL_MAX_OUTPUT_TOKENS`,
  `FACTORY_LEAD_MODEL_MAX_COST_MICROS`
- `FACTORY_REVIEW_MODEL_MAX_INPUT_TOKENS`,
  `FACTORY_REVIEW_MODEL_MAX_OUTPUT_TOKENS`,
  `FACTORY_REVIEW_MODEL_MAX_COST_MICROS`
- `FACTORY_EVALUATION_MODEL_MAX_INPUT_TOKENS`,
  `FACTORY_EVALUATION_MODEL_MAX_OUTPUT_TOKENS`,
  `FACTORY_EVALUATION_MODEL_MAX_COST_MICROS`
- `FACTORY_ENABLE_AUTONOMOUS_START` and `FACTORY_ALLOW_TEST_UPSTREAM`

`FACTORY_MODEL_URL` and a token of at least 16 bytes are required. Plain HTTP is
accepted only for a loopback test upstream with
`FACTORY_ALLOW_TEST_UPSTREAM=1`.

`FACTORY_PROVIDER_ENV_FILE` is parsed by the CLI and only the parsed values are
added to the host Worker process environment. It is never passed to the Core
container, the Podman command line or the Supervisor. Its allowlist is:

- `FACTORY_PROVIDER_SANDBOX`, `FACTORY_PROVIDER_KIND`,
  `FACTORY_PROVIDER_BINARY`, `FACTORY_PROVIDER_ARGS`,
  `FACTORY_PROVIDER_SESSION_ROOT`, `FACTORY_PROVIDER_SESSION_MAX_BYTES`,
  `FACTORY_PROVIDER_MEMORY_LIMIT_BYTES`, `FACTORY_PROVIDER_MODEL_CONTEXT`,
  `FACTORY_PROVIDER_MODEL_OUTPUT`, `FACTORY_PROVIDER_TCP_PORT`,
  `FACTORY_PROVIDER_KEEP_SESSION`
- `FACTORY_PROVIDER_BRIDGE`, `FACTORY_PROVIDER_EGRESS_BRIDGE`,
  `FACTORY_PROVIDER_EGRESS_TOKEN`, `FACTORY_PROVIDER_UPSTREAM_URL`,
  `FACTORY_PROVIDER_UPSTREAM_TOKEN`
- `FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION`,
  `FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION`
- `FACTORY_PROVIDER_BASE_URL`, `FACTORY_PROVIDER_PASSWORD`,
  `FACTORY_ALLOW_UNCONFINED_PROVIDER` and `FACTORY_ALLOW_TEST_UPSTREAM`

Provider configuration requires `FACTORY_HOST_EXECUTION=1` and at least one
`FACTORY_PROJECT_ROOTS` entry, because the configured values are delivered to
the host Worker. A configured route profile does not authorize a Campaign or
raise a budget; Core authorization remains mandatory. Both the OpenCode and
Codex route profiles use this Worker-only allowlist. The Codex profile requires
both explicit input/output cost rates. OpenCode has a managed live acceptance
record; the Codex adapter is implemented and deterministic-tested. The
only supported Codex live authentication mode is an operator API key at
`https://api.openai.com/v1`; live Codex inference has not been run.
Native ChatGPT OAuth is not supported by the current egress boundary; see
[native/README.md](../native/README.md#codex-app-server-provider-trusted_local_provider).

`factory doctor` and `factory start` validate both files before any Podman
container change. A running Core container carries a
`factory.core-model-env-fingerprint` label. If the configured Core model env
content changes, or the label is absent or different, `start` stops and
recreates the container deterministically instead of reusing stale
configuration. `start` and `status` print only redacted route/profile
summaries; fingerprints and token-presence markers are not credentials.

```sh
install -m 600 /dev/null "$HOME/.config/factory/model.env"
install -m 600 /dev/null "$HOME/.config/factory/provider.env"
export FACTORY_CORE_MODEL_ENV_FILE="$HOME/.config/factory/model.env"
export FACTORY_PROVIDER_ENV_FILE="$HOME/.config/factory/provider.env"
export FACTORY_PROJECT_ROOTS=/absolute/path/to/project
factory doctor
factory start
factory status
```

Do not commit either file or place credentials in this repository.

## Status

Build the image with the command in [`deploy/README.md`](../deploy/README.md).
On the tested Linux host with rootless Podman 6.1.4 and pasta available,
`factory start` completed with the real OpenCode provider profile: Core health
200, authenticated API and Rust Office accessible only on `127.0.0.1`, host
Supervisor/Worker running, real model Lead and Implementer inference, a
rootless-Podman verifier, semantic Reviewer and `ACCEPTED` gate, plus restart
persistence and clean stop. A separate test-only managed self-development
candidate also reached `ACCEPTED`. Windows, production deployment and
isolation certification are not claimed. Evidence:
[managed live UAT](../docs/evidence/managed-live-uat-2026-10-26.md).
