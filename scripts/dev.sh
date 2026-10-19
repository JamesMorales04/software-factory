#!/usr/bin/env bash
# Local S1 development: builds the Office UI, then serves Core + API + UI
# together (Core serves ./office/dist). One token for both; printed once.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export FACTORY_API_TOKEN="${FACTORY_API_TOKEN:-dev-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')}"

echo "== office build =="
(cd "$ROOT/office" && npm install --no-audit --no-fund && npm run build)

echo "== core (token below; Factory Office will ask for it) =="
echo "FACTORY_API_TOKEN=$FACTORY_API_TOKEN"
(cd "$ROOT/core" && go run ./cmd/factory-core --addr 127.0.0.1:8471 --db "$ROOT/data/factory.db" --web-dir "$ROOT/office/dist")
