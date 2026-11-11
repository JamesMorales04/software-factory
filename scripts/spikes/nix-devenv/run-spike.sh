#!/bin/sh
# Runs inside the spike image. No host binaries are used.
set -eu

mode="${1:-all}"
offline="${OFFLINE:-0}"

export npm_config_cache="${npm_config_cache:-/cache/npm}"
export CARGO_HOME="${CARGO_HOME:-/cache/cargo}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/cache/target}"
mkdir -p "$npm_config_cache" "$CARGO_HOME" "$CARGO_TARGET_DIR"

cd /spike

nix_develop() {
  shell="$1"
  shift
  if [ "$offline" = "1" ]; then
    nix develop --offline ".#$shell" --command "$@"
  else
    nix develop ".#$shell" --command "$@"
  fi
}

run_versions() {
  nix_develop node sh -c '
    node --version
    npm --version
    printf "PLAYWRIGHT_BROWSERS_PATH=%s\n" "$PLAYWRIGHT_BROWSERS_PATH"
    ls "$PLAYWRIGHT_BROWSERS_PATH"
  '
  nix_develop rust sh -c '
    rustc --version
    cargo --version
    clippy-driver --version
  '
}

run_node() {
  nix_develop node bash -lc '
    set -eu
    cd projects/node
    node --version
    npm --version
    if [ "${OFFLINE:-0}" = "1" ]; then
      npm ci --offline --no-audit --no-fund
    else
      npm ci --no-audit --no-fund
    fi
    npm test
    npm run measure
  '
}

run_rust() {
  nix_develop rust bash -lc '
    set -eu
    cd projects/rust
    rustc --version
    cargo --version
    if [ "${OFFLINE:-0}" = "1" ]; then
      cargo test --locked --offline
    else
      cargo test --locked
    fi
  '
}

run_sbom() {
  mkdir -p /cache/sbom
  nix_develop tools bash -lc '
    set -eu
    sbomnix .#devShells.x86_64-linux.node \
      --cdx /cache/sbom/node.cdx.json --csv /cache/sbom/node.csv
    sbomnix .#devShells.x86_64-linux.rust \
      --cdx /cache/sbom/rust.cdx.json --csv /cache/sbom/rust.csv
    wc -c /cache/sbom/node.cdx.json /cache/sbom/rust.cdx.json
  '
}

case "$mode" in
  versions) run_versions ;;
  node) run_node ;;
  rust) run_rust ;;
  sbom) run_sbom ;;
  all)
    run_versions
    run_node
    run_rust
    ;;
  *)
    echo "usage: run-spike [versions|node|rust|sbom|all]" >&2
    exit 2
    ;;
esac
