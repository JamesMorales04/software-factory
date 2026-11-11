# Nix/devenv environment-compiler spike — 2026-11-10

**Status:** Implementer spike evidence (`LOCALLY_VERIFIED` for the exact
commands below). **This is not independent verification, product acceptance,
isolation certification or a production runtime change.** The spike is
time-boxed: the flake path completed cold, warm and offline-warm runs; the
devenv shell was evaluated but its full cold activation did not complete and is
recorded as `NOT_TESTED`.

**Scope:** Prove that a pinned Nix flake can materialize a Node 22 + npm +
Playwright/Chromium environment and a Rust 1.90 environment inside rootless
Podman without host Nix/Node/npm/Cargo/Playwright, and record the limits,
licenses and EnvironmentPlan implications. The spike also contains a devenv
declaration and lock; the flake path is the fully executed path.

**Candidate:** spike branch `spike/nix-devenv-environment` based on
`ba2dda9`; the evidence commit is recorded in the handoff.

**Non-claims:** this record does not establish any of the following:

- that the full Factory Rust workspace, `cargo xtask check`, Office, Compose or
  a real browser UAT works under Nix;
- that Nix/devenv replaces the canonical Compose development image today;
- that the environment is isolated or suitable for untrusted execution;
- that Chromium is sandboxed (the spike runs it with `--no-sandbox`);
- that every license in the generated SBOM is compatible with Factory
  distribution policy;
- Windows or arm64 support;
- that Devenv Cloud or any paid service was used.

No paid inference or paid service was used to produce this document.

## 1. Environment

| Item | Recorded value |
|---|---|
| Host | CachyOS Linux, kernel 7.3.1-1-cachyos, x86_64 |
| Container runtime | Rootless Podman 6.1.4, overlay storage, user namespace |
| Container image | `docker.io/nixos/nix:2.28.3` at `sha256:e623d73af9cac82d1b50784c83e0cf2a4b83bfd2cfe8d5b67809a2fc94e043ac` |
| Host Nix/devenv | absent; not installed for the spike |
| Host Node/Cargo/Playwright | not used by the spike; the spike image provides its own |
| Network | bridge for the cold run; `none` for the offline warm run |
| Host mounts | none; only Podman named volumes for `/nix` and `/cache` |
| Privileges | no `--privileged`, no added capabilities, `no-new-privileges` |
| Host networking | no; Podman bridge or `none` only |

The host already had `mise`-managed Node and Cargo for unrelated work. They were
not invoked by the spike, are not copied into the image, and are not required to
reproduce it. The only host prerequisite exercised is rootless Podman.

## 2. Exact pins

| Input | Pin |
|---|---|
| Nix | 2.28.3 from the pinned base image |
| nixpkgs | `4fe25e12eb9f4c744966dd3de5a05246cf098601` (`sha256-WrSODdZFVIs/Hh2NzC0bY+ROByi0bATg8WfePX8dPj8=`) |
| rust-overlay | `89750a2ef1c6a44db9f4251b16c23deffdcfddff` (`sha256-UZMtOt43/HRVssXPSuPv8t11Toq9+CjyWUfYQ3tqGVE=`) |
| devenv modules | `cachix/devenv@4a1138b49c3b0503c214e4b6e59102dbdd3282ca` (`sha256-ks05AJY27rCF6KzuXIlmky0eZzNGgbSTfHKTr4j2rdg=`) |
| Node | 22.23.4 (nixpkgs `nodejs_22`; repository `.nvmrc` is 22.23.2) |
| npm | 10.9.9 |
| Playwright | 1.64.1 (exact repository pin) |
| Chromium | Playwright revision 1248 |
| Rust | 1.90.0 (`1159e78c4`), cargo 1.90.0, clippy 0.1.90 |

`flake.lock` and `devenv.lock` are committed under
`scripts/spikes/nix-devenv/`. The Nix store paths are content-addressed by the
lock inputs; the recorded store paths in this document are evidence for this
run, not a portable ABI.

## 3. Spike layout and reproduction

The spike lives in [`scripts/spikes/nix-devenv`](../../scripts/spikes/nix-devenv/README.md).

| Path | Purpose |
|---|---|
| `Containerfile` | Pinned Nix base, spike sources, no host toolchain |
| `flake.nix` / `flake.lock` | `node`, `rust` and `tools` devShells |
| `devenv.nix` / `devenv.yaml` / `devenv.lock` | Evaluated devenv alternative |
| `projects/node` | Node 22 + npm + Playwright/Chromium test and browser timing |
| `projects/rust` | Rust 1.90 library test with a crates.io dependency |
| `run.sh` | Host wrapper: rootless Podman, named volumes only |
| `run-spike.sh` | In-container modes: `versions`, `node`, `rust`, `sbom`, `all` |
| `evidence` | Cold-run log/timing, devenv info, SBOM summary |

Reproduction from the repository root:

```sh
cd scripts/spikes/nix-devenv
RESET=1 ./run.sh all          # cold: fresh /nix and /cache volumes
./run.sh all                  # warm: reuse the volumes
OFFLINE=1 NETWORK=none ./run.sh all   # offline warm, after a cold run
./run.sh sbom                 # CycloneDX + CSV into the cache volume
```

No host Nix, Node, npm, Cargo, Playwright or devenv command is used. `run.sh`
builds the image, creates two named volumes and runs the container. The
projects are copied into the image; there is no source bind mount.

## 4. Results

### 4.1 Node 22 + npm + Playwright/Chromium

The final Node environment reported:

```text
v22.23.4
10.9.9
/nix/store/xzcwph37l5pv0r3gj3di4g35dlwrzazf-playwright-browsers
chromium-1248
chromium_headless_shell-1248
ffmpeg-1013
```

The Playwright test launches Chromium, sets page content, executes JavaScript
and asserts the rendered value. Three completed runs:

| Run | Playwright test | Browser launch / new page / render |
|---|---|---|
| Cold | `1 passed (949ms)` | 61 ms / 42 ms / 43 ms |
| Warm | `1 passed (551ms)` | 47 ms / 26 ms / 31 ms |
| Offline warm | `1 passed (578ms)` | 40 ms / 28 ms / 45 ms |

These are single-host measurements, not a performance benchmark. The test uses
`--no-sandbox` because the container runs as mapped root; this is **not** a
browser-sandbox claim.

### 4.2 Rust 1.90

```text
rustc 1.90.0 (1159e78c4 2025-09-14)
cargo 1.90.0 (840b83a10 2025-07-30)
clippy 0.1.90 (1159e78c47 2025-09-14)
```

```text
     Running unittests src/lib.rs (/cache/target/debug/deps/wp0_rust_spike-...)

running 1 test
test tests::renders_expected_json ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The cold run compiled the crate in 3.20 s; warm and offline runs finished the
test in 0.02 s from the cache. `cargo fmt --check` and
`cargo clippy --locked -- -D warnings` also pass for the trivial crate. This
proves the Rust toolchain and a crates.io dependency work; it does not prove the
Factory workspace's SQLx/OpenSSL/SQLite/bubblewrap dependencies.

### 4.3 Cold and warm materialization

| Run | Result |
|---|---|
| Cold `./run.sh all` with fresh `/nix` and `/cache` | 1,422,745 ms (23 m 42.7 s), exit 0 |
| Warm `./run.sh all` with reused volumes | 18,214 ms (18.2 s), exit 0 |
| Offline warm, `OFFLINE=1 NETWORK=none` | 18,944 ms (18.9 s), exit 0 |
| Spike image size | 770 MB (769,664,520 bytes; Nix 2.28.3 base plus sources) |
| Cached image rebuild | 19.5 s |
| Nix store after cold Node+Rust | 4.8 GB |
| Nix store after adding `sbomnix`/tools | 5.8 GB |
| Cache volume after cold + SBOM | 51 MB |
| Node devShell closure | 1.4 GiB |
| Rust devShell closure | 1.8 GiB |
| `flake.lock` evaluation (warm) | ~3.7 s for `.#node`; ~4.4 s for `.#rust` |
| Cold Rust shell first materialization (separate earlier run) | 12 m 41 s |
| `sbomnix` on the cold image, both devShells | 970,078 ms (16 m 10 s) |
| `sbomnix` on `.#devShells...node` (warm dev volume) | 11.6 s |
| `sbomnix` on `.#devShells...rust` (warm dev volume) | 10.4 s |

The cold run starts with an already-pulled base image and a cached Containerfile
build; its 23 m 43 s is dominated by `cache.nixos.org` downloads and Rust
toolchain materialization. Warm and offline runs re-use the same `/nix` and
`/cache` volumes and do not re-download the same store paths. The 12 m 41 s
Rust-shell number is an earlier isolated measurement; the combined cold run is
the authoritative end-to-end result.

### 4.4 Devenv evaluation

`devenv update` generated `devenv.lock` and validated the nixpkgs,
rust-overlay and implicit devenv module inputs. `devenv info` evaluates the
declaration and reports:

```text
PLAYWRIGHT_BROWSERS_PATH: /nix/store/xzcwph37l5pv0r3gj3di4g35dlwrzazf-playwright-browsers
nodejs-22.23.4
rust-stable-1.90.0-1.90.0
rust-analyzer-preview-1.90.0
clang-wrapper-21.1.8
valgrind-3.27.1
gdb-17.2
ccls-0.20250815.1
sbomnix-1.9.0
```

The full log is `evidence/devenv-info.log`. The declaration uses
`languages.javascript` (not `languages.nodejs`, which does not exist in devenv
2.4.0), `languages.rust` with `channel = "stable"` and `version = "1.90.0"`,
and an explicit `rust-overlay` input.

Full cold `devenv shell` activation is **`NOT_TESTED`**. It was attempted with:

```sh
devenv --no-tui shell -- sh -c 'node --version; rustc --version'
devenv --no-tui shell -- sh -c 'cd projects/node && npm test && npm run measure'
```

Both attempts were still downloading/building a large opinionated closure
(grype, vulnix, valgrind, gdb, clang tools, Node 24, pnpm, Perl modules and
related paths) after 20–30 minutes, so the spike was time-boxed and stopped.
`devenv update` and `devenv info` passed. The flake devShells executed fully for
both Node/Playwright and Rust. This is a partial result, not a passing devenv
shell test.

### 4.5 SBOM and license metadata

`sbomnix` 1.9.0 can generate CycloneDX 1.4 and CSV from the flake devShells.
The target must be a **flake reference**; a bare Nix store path produces an
SBOM without nixpkgs license metadata. The recorded warm runs produced:

| Target | Components | Components without license | Time |
|---|---:|---:|---:|
| `.#devShells.x86_64-linux.node` | 158 | 30 | 11.6 s |
| `.#devShells.x86_64-linux.rust` | 61 | 26 | 10.4 s |

A cold-image `./run.sh sbom` on the same volumes took 970,078 ms (16 m 10 s),
including the first download of `sbomnix` and its tools. It produced a
280,410-byte Node CycloneDX file and a 94,695-byte Rust CycloneDX file with the
same component/license counts.

Top license families in the Node closure included MIT (33), GPL-2.0-or-later
(17), BSD-3-Clause (16), GPL-3.0-or-later (15), LGPL-2.1-or-later (14),
Apache-2.0 (7), and MPL-2.0 (3). The Rust closure included GPL-3.0-or-later
(14), GPL-2.0-or-later (7), LGPL-2.1-or-later (4), BSD-3-Clause (4), MIT (3),
and Zlib (1). The full summary is `evidence/sbom-summary.txt`.

License findings for the compiler inputs themselves:

| Component | License | Evidence |
|---|---|---|
| Nix 2.28.3 | LGPL-2.1 | upstream `COPYING`; nixpkgs `nix.meta.license` is `LGPL-2.1-or-later` |
| nixpkgs | MIT | upstream `COPYING` |
| devenv | Apache-2.0 | upstream `LICENSE`; nixpkgs `devenv.meta.license` is `asl20` |
| Node.js 22 | MIT | nixpkgs `nodejs_22.meta.license` |
| Playwright | Apache-2.0 | nixpkgs `playwright-driver.meta.license`; the browser link farm itself lacks SPDX metadata |
| Rust toolchain | MIT / Apache-2.0 (upstream) | upstream Rust license; rust-overlay derivations do not expose `meta.license`, so `sbomnix` cannot attribute them |

This is metadata, not a distribution-policy conclusion. The generated closure
contains GPL and LGPL binaries and libraries; shipping a container built from
that closure needs a Factory license review, notices/source-offer handling, and
a decision on whether build/runtime closures are in scope. The SBOM also mixes
SPDX identifiers and nixpkgs short names and omits metadata for a material
number of components; a Factory normalizer must fail closed on `UNKNOWN` or
require review.

The missing metadata is not only incidental build tools. In the Rust SBOM,
`rustc`, `cargo`, `rust-std`, `rust-docs`, `clippy-preview`, `rustfmt-preview`
and the `rust-default` wrapper have no SPDX identifier. In the Node SBOM, the
Playwright browser link farm (`playwright-browsers`), `playwright-chromium`,
`playwright-chromium-headless-shell`, `binutils`, `gmp`, `linux-headers` and
the font configuration are also missing. The Rust toolchain licenses must be
recorded from the upstream MIT/Apache-2.0 terms (or an explicit overlay
metadata patch); the Chromium/Playwright browser licenses must be recorded from
the upstream distribution terms. `sbomnix` alone does not close that gap.

No paid service was used. Devenv's default public `devenv.cachix.org` binary
cache is optional; it is not Devenv Cloud and no account was created. The flake
path uses only `cache.nixos.org`, GitHub, `registry.npmjs.org` and `crates.io`.

## 5. Failure modes and limitations

| Failure mode | Observed/expected | Mitigation |
|---|---|---|
| `NIX_CONFIG` with a literal `\n` | Disabled `flakes`; run failed before evaluation | Use a single-line value or `/etc/nix/nix.conf` |
| Full Playwright browser set | Pulls Chromium, headless shell, Firefox, WebKit and a large media closure | `browsers.override { withFirefox = false; withWebkit = false; }` |
| Chromium as mapped root | Needs `--no-sandbox` | Run as non-root with a working browser sandbox before claiming browser isolation |
| Cold cache throughput | Some `cache.nixos.org` NARs took hundreds of seconds; the combined cold Node+Rust run took 23 m 43 s | Budget cold materialization, mirror/cache, or pre-bake the store |
| devenv shell closure | Large opinionated tools and public Cachix cache; full cold shell did not finish in 20–30 min (`NOT_TESTED`) | Use the flake for the compiled contract or trim devenv modules |
| SBOM on a store path | No nixpkgs license metadata | Use a flake reference; keep the generated CSV for SPDX IDs |
| Missing license metadata | 30/158 Node and 26/61 Rust components | Treat as `UNKNOWN`; fail closed or require review |
| Exact Node patch pin | nixpkgs at the Playwright-1.64.1 revision provides Node 22.23.4; the Node 22.23.2 revision provides Playwright 1.61.1 | Accept patch drift policy, use multiple pinned nixpkgs inputs, or add a custom Node overlay |
| Network failure | Cold run cannot materialize a new store | Warm/offline mode works only after all inputs and caches are present |
| Full workspace | Not tested | Add SQLx CLI, cargo-audit, SQLite/OpenSSL, bubblewrap and Office dependencies before claiming parity |

## 6. Existing repository conventions

| Existing convention | Followable with Nix? | Evidence/limit |
|---|---|---|
| `rust-toolchain.toml` pins Rust 1.90.0 | Yes, exactly | rust-overlay `stable."1.90.0"`; devenv `languages.rust.version` or `toolchainFile` |
| `.nvmrc` pins Node 22.23.2 | Major/minor yes; exact patch not in the chosen nixpkgs rev | nixpkgs provides 22.23.4; exact patch needs a matching rev or overlay |
| `mise.toml` optional host convenience | Not needed for container-native development | Nix devShell replaces the host tool shim; no need to keep mise in the product path |
| `.devcontainer/Containerfile` | Not replaced in this spike | Nix can generate a devShell or image; devcontainer integration is `NOT_TESTED` |
| `compose.yaml` product stack | Not changed | The spike is not a Compose service and has no host socket |
| `cargo xtask check` | Not tested on the full workspace | The flake Rust shell has rustc/cargo/clippy/rustfmt, not SQLx CLI, cargo-audit, SQLite/OpenSSL/bubblewrap |
| Existing flake/devenv/asdf | None in the repository | The spike introduces the first flake/devenv files as a spike, not a product convention |

The repository's existing `mise.toml` and `.nvmrc` are not blocking. The main
version gap is the exact Node patch; the main tooling gap is the Factory
workspace's native dependencies and audit/SQLx binaries.

## 7. Recommended EnvironmentPlan fields

The spike supports a deterministic, Core-owned plan. Model output must not
grant authority or choose tools. Recommended fields:

| Field | Type/example | Why |
|---|---|---|
| `schema_version` | `1` | Stable plan schema |
| `plan_id` / `content_hash` | UUID / SHA-256 | Bind authorization and evidence to the exact plan |
| `project_id`, `workspace_id` | Factory identifiers | Scope and authorization |
| `platform` | `linux/x86_64` | Nix system; no implicit host fallback |
| `compiler.kind` | `nix-flake` | One canonical compiler; no per-language image |
| `compiler.flake_ref` | `path:...#devShells.x86_64-linux.node` | Exact output selector |
| `compiler.inputs[]` | `{name, url, rev, nar_hash, follows}` | Reproducible nixpkgs/rust-overlay/devenv pins |
| `compiler.lock_digest` | SHA-256 of `flake.lock` | Detect lock drift |
| `compiler.devenv_lock_digest` | SHA-256 of `devenv.lock` (optional authoring lock) | Detect devenv authoring drift |
| `toolchains[]` | `{language, version, package_attr, components, store_path, output_hash}` | Exact Node/Rust/npm/Playwright versions |
| `browsers[]` | `{name: chromium, revision: 1248, store_path, launch_args}` | Browser capability is separate from Node |
| `native_dependencies[]` | `{name, version, store_path}` | SQLite, OpenSSL, pkg-config, etc. |
| `environment` | `{PLAYWRIGHT_BROWSERS_PATH, CARGO_HOME, ...}` | Deterministic runtime env |
| `commands` | `{build, test, lint, format, audit}` | The plan's executable contract |
| `cache_policy` | `{substituters, trusted_public_keys, named_volumes, offline_after_warm}` | Cold/warm behavior and network budget |
| `network_policy` | `{cold_endpoints[], warm_offline: true}` | Egress request binding and no hidden downloads |
| `license_policy` | `{sbom_format: CycloneDX, allow[], deny[], unknown: fail_closed}` | License decision is Factory policy |
| `sbom` | `{path, digest, generator_version}` | Candidate-bound evidence |
| `resources` | `{cpu, memory, disk, timeout}` | Bounded execution |
| `trust_class` | `TRUSTED_LOCAL` development / ephemeral execution | No isolation claim |
| `verification` | `{commands, expected_hashes, negative_cases}` | Acceptance is deterministic |
| `unknowns` / `not_tested` | explicit list | Never turn a green subset into a certification |

Illustrative shape (values shortened; store paths are run-specific):

```json
{
  "schema_version": 1,
  "plan_id": "00000000-0000-0000-0000-000000000000",
  "content_hash": "sha256:...",
  "platform": "x86_64-linux",
  "compiler": {
    "kind": "nix-flake",
    "flake_ref": "path:/workspace#devShells.x86_64-linux.node",
    "lock_digest": "sha256:...",
    "inputs": [
      {
        "name": "nixpkgs",
        "url": "github:NixOS/nixpkgs",
        "rev": "4fe25e12eb9f4c744966dd3de5a05246cf098601",
        "nar_hash": "sha256-WrSODdZFVIs/Hh2NzC0bY+ROByi0bATg8WfePX8dPj8="
      },
      {
        "name": "rust-overlay",
        "url": "github:oxalica/rust-overlay",
        "rev": "89750a2ef1c6a44db9f4251b16c23deffdcfddff",
        "nar_hash": "sha256-UZMtOt43/HRVssXPSuPv8t11Toq9+CjyWUfYQ3tqGVE="
      }
    ]
  },
  "toolchains": [
    {
      "language": "node",
      "version": "22.23.4",
      "package_attr": "nodejs_22",
      "store_path": "/nix/store/...-nodejs-22.23.4",
      "components": ["node", "npm"]
    },
    {
      "language": "rust",
      "version": "1.90.0",
      "package_attr": "rust-bin.stable.1.90.0.default",
      "store_path": "/nix/store/...-rust-default-1.90.0",
      "components": ["rustc", "cargo", "clippy", "rustfmt"]
    }
  ],
  "browsers": [
    {
      "name": "chromium",
      "playwright_version": "1.64.1",
      "revision": "1248",
      "store_path": "/nix/store/...-playwright-browsers",
      "launch_args": ["--no-sandbox"]
    }
  ],
  "cache_policy": {
    "substituters": ["https://cache.nixos.org"],
    "named_volumes": ["/nix", "/cache"],
    "offline_after_warm": true
  },
  "network_policy": {
    "cold_endpoints": [
      "cache.nixos.org",
      "github.com",
      "registry.npmjs.org",
      "crates.io"
    ]
  },
  "license_policy": {
    "sbom_format": "CycloneDX-1.4",
    "unknown": "fail_closed"
  },
  "trust_class": "TRUSTED_LOCAL",
  "not_tested": ["full-factory-workspace", "office", "compose-e2e"]
}
```

Recommended canonical shape:

1. **Compile from a pinned flake**, not from a mutable toolchain image. Record
   `flake.lock`, the resolved store paths and the SBOM digest in the plan.
2. **Treat devenv as an authoring front-end, not a second compiler.** If the
   project authors `devenv.nix`, resolve it to the same pinned flake/lock and
   verify the resulting store paths. Do not maintain independent flake and
   devenv environments that can drift.
3. **Pin each exact toolchain source.** One nixpkgs revision may not satisfy
   every exact patch version (Node 22.23.2 vs Playwright 1.64.1 is the observed
   example). The plan must record per-toolchain source pins and an explicit
   patch-drift policy.
4. **Use Chromium-only Playwright browsers** unless Firefox/WebKit are required.
   This removes a large media closure.
5. **Make cold network and cache behavior explicit.** A cold plan needs
   `cache.nixos.org` (and the package registries for npm/crates.io unless
   vendored); a warm plan can run offline. Budget the cold time and disk.
6. **Generate an SBOM and fail closed on unknown licenses.** Do not treat
   `sbomnix` output as an automatic license approval.
7. **Do not claim sandbox isolation.** The browser runs with `--no-sandbox` in
   this spike; the container is `TRUSTED_LOCAL` development, not an execution
   sandbox.

This is a builder recommendation, not a product decision. If the owner
standardizes on `devenv.nix` as the canonical authoring surface, the same plan
fields apply; the invariant is that the resolved lock files and store paths,
not the mutable shell, are the contract. Do not maintain independent flake and
devenv environments that can drift.

## 8. NOT_TESTED / blocked

- **Full Factory workspace under Nix** — `NOT_TESTED`.
- **`cargo xtask check` in a Nix devShell** — `NOT_TESTED`; native dependencies
  and SQLx/cargo-audit are not in the spike shell.
- **Office build/test and Compose E2E under Nix** — `NOT_TESTED`.
- **Full `devenv shell` activation** — `NOT_TESTED`; attempted twice, still
  downloading/building after 20–30 minutes when the spike was time-boxed.
  `devenv info` and `devenv update` passed.
- **Full Rust workspace under the devenv shell** — `NOT_TESTED`; the Rust flake
  shell completed, but the devenv-based Rust shell did not reach execution.
- **Offline cold start** — `NOT_APPLICABLE`; a cold store cannot be populated
  without a source.
- **Windows and arm64** — `NOT_TESTED`; the flake declares `aarch64-linux` but
  only x86_64 was run.
- **Digest pinning of every flake input** — `IMPLEMENTED` for nixpkgs,
  rust-overlay and devenv via `flake.lock`/`devenv.lock`; GitHub tarball
  transport is not independently mirrored.
- **Independent verification** — not performed; this is implementer evidence.
- **Paid services / Devenv Cloud** — none used.

## 9. Reproduction record

Raw logs and timings:

- [`evidence/container-cold.log`](../../scripts/spikes/nix-devenv/evidence/container-cold.log)
- [`evidence/container-cold.time`](../../scripts/spikes/nix-devenv/evidence/container-cold.time)
- [`evidence/container-warm.log`](../../scripts/spikes/nix-devenv/evidence/container-warm.log)
- [`evidence/container-warm.time`](../../scripts/spikes/nix-devenv/evidence/container-warm.time)
- [`evidence/container-offline.log`](../../scripts/spikes/nix-devenv/evidence/container-offline.log)
- [`evidence/container-offline.time`](../../scripts/spikes/nix-devenv/evidence/container-offline.time)
- [`evidence/container-sbom.log`](../../scripts/spikes/nix-devenv/evidence/container-sbom.log)
- [`evidence/container-sbom.time`](../../scripts/spikes/nix-devenv/evidence/container-sbom.time)
- [`evidence/devenv-info.log`](../../scripts/spikes/nix-devenv/evidence/devenv-info.log)
- [`evidence/sbom-summary.txt`](../../scripts/spikes/nix-devenv/evidence/sbom-summary.txt)

The cold run was executed on 2026-11-10 with `RESET=1 ./run.sh all`. The
`container-cold.time` file records the elapsed wall time and exit status. Warm
and offline runs were executed with `./run.sh all` and
`OFFLINE=1 NETWORK=none ./run.sh all` after the cold run. The SBOM run used
`./run.sh sbom`; the raw CycloneDX/CSV files remain in the named cache volume
(`wp0-spike-cache`) because they are machine-generated artifacts, while
`evidence/sbom-summary.txt` records the counts. Commands are reproducible from
the spike README; they do not use host language toolchains.
