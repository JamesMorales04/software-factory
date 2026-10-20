#!/usr/bin/env bash
# Build a local distribution: compiled Go CLI/Core plus the production Office.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/build/factory}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

echo "== Factory Office =="
(cd "$ROOT/office" && npm run build)

echo "== Factory binaries =="
(cd "$ROOT/core" && go build -o "$OUT_DIR/factory-core" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$OUT_DIR/factory" ./cmd/factory)
(cd "$ROOT/core" && go build -o "$OUT_DIR/factory-worker" ./cmd/factory-worker)
(cd "$ROOT/core" && go build -o "$OUT_DIR/factory-verifier" ./cmd/factory-verifier)
(cd "$ROOT/core" && go build -o "$OUT_DIR/factory-reviewer" ./cmd/factory-reviewer)

echo "== Office distribution =="
mkdir -p "$OUT_DIR/office"
rm -rf "$OUT_DIR/office/dist"
cp -R "$ROOT/office/dist" "$OUT_DIR/office/dist"

echo "Factory CLI: $OUT_DIR/factory"
