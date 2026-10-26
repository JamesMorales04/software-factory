#!/bin/sh
set -eu

case "${FACTORY_PROVIDER_KIND:-}" in
  fixture)
    exec /usr/local/bin/factory-fixture-provider
    ;;
  opencode)
    /usr/local/bin/factory-egress-bridge &
    /usr/local/bin/factory-provider-bridge &
    exec /usr/local/bin/opencode serve \
      --hostname 127.0.0.1 \
      --port 18081 \
      --pure
    ;;
  codex)
    echo "factory-provider: containerized Codex stdio transport is not implemented" >&2
    exit 78
    ;;
  *)
    echo "factory-provider: unsupported FACTORY_PROVIDER_KIND" >&2
    exit 78
    ;;
esac
