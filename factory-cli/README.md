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
| `FACTORY_PROJECT_ROOTS` | unset; explicit comma-separated absolute Git roots |
| `FACTORY_SUPERVISOR_URL` | `http://127.0.0.1:8766` |
| `FACTORY_BIN_DIR` | directory of the running `factory` binary |
| `FACTORY_SUPERVISOR_IMAGES` | `docker.io/library/alpine:3.20` |

`FACTORY_CORE_URL` must be an HTTP origin on IPv4 loopback with an explicit
port. `start` publishes only `127.0.0.1:<port>`.

## Status

Build the image with the command in [`deploy/README.md`](../deploy/README.md).
On the CachyOS host with kernel `7.3.0-3-cachyos`, `tun` loaded, rootless
Podman 6.1.4 and pasta available, `factory start` completed: container created,
published only on `127.0.0.1`, Core health 200, authenticated API accessible,
the Rust Office served from the image, and a durable Worker registration
survived stop/start. Windows and isolation certification are not claimed.
