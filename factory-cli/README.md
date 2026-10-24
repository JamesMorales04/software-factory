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
Podman socket, then waits for `/health`. It generates four distinct role
tokens into a private `0600` env file if absent and never prints token values.

`stop` keeps the data volume unless `--purge` is given and explicitly confirmed
by typing `purge` on a terminal. `logs` follows `podman logs`. `status` reports
container state and Core health. The printed Office URL is the loopback Core
API origin; the React Office is not packaged in this image.

## Configuration

| Variable | Default |
|---|---|
| `FACTORY_CORE_URL` | `http://127.0.0.1:8765` |
| `FACTORY_IMAGE` | `localhost/software-factory-core:dev` |
| `FACTORY_CONTAINER_NAME` | `factory-core` |
| `FACTORY_VOLUME` | `factory-core-data` |
| `FACTORY_ENV_FILE` | `$XDG_CONFIG_HOME/factory/core.env` or `$HOME/.config/factory/core.env` |

`FACTORY_CORE_URL` must be an HTTP origin on IPv4 loopback with an explicit
port. `start` publishes only `127.0.0.1:<port>`.

## Status

Build the image with the command in [`deploy/README.md`](../deploy/README.md).
On the CachyOS host with kernel `7.3.0-3-cachyos`, `tun` loaded, rootless
Podman 6.1.4 and pasta available, `factory start` completed: container created,
published only on `127.0.0.1`, Core health 200, authenticated API accessible,
the Rust Office served from the image, and a durable Worker registration
survived stop/start. Windows and isolation certification are not claimed.
