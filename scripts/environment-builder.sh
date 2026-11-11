#!/usr/bin/env bash
# Rootless Podman wrapper for the generic Nix/devenv environment builder.
#
# This is a development/CI harness. The installed product must invoke the same
# image through the runtime broker in the canonical Compose topology; it must
# not gain a host toolchain, a host socket or a host bind beyond the explicit
# read-only project and writable cache mount below.
#
# Usage:
#   scripts/environment-builder.sh build --request REQUEST.json --workspace DIR --output DIR [--fetch]
#   scripts/environment-builder.sh fetch --request REQUEST.json --workspace DIR --output DIR
#   scripts/environment-builder.sh check --request CHECK.json --workspace DIR --output DIR
#   scripts/environment-builder.sh descriptor
#
# Request/check JSON uses the Core EnvironmentSpec/EnvironmentPlan shape plus
# optional `workspace`, `runDir`, `fetchProxy` and `check` fields.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
DIGEST="${FACTORY_ENVIRONMENT_BUILDER_DIGEST:-}"
PLATFORM="${FACTORY_ENVIRONMENT_BUILDER_PLATFORM:-x86_64-linux}"
POLICY_REVISION="${FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION:-nix-devenv-environment-v1}"
CACHE="${FACTORY_ENVIRONMENT_BUILDER_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/software-factory/environment-builder}"
FETCH_NETWORK="${FACTORY_ENVIRONMENT_BUILDER_FETCH_NETWORK:-slirp4netns}"
FETCH_PROXY="${FACTORY_ENVIRONMENT_BUILDER_FETCH_PROXY:-}"

log() { printf '%s\n' "$*" >&2; }

typed_failure() {
  python3 - "$1" "$2" <<'PY'
import json, sys
print(json.dumps({"status": "FAILED", "blocker": {"code": sys.argv[1], "message": sys.argv[2]}}, sort_keys=True))
PY
  exit 2
}

resolve_digest() {
  local resolved="$DIGEST"
  if [[ -z "$resolved" ]]; then
    if ! podman image exists "$IMAGE" >/dev/null 2>&1; then
      return 1
    fi
    resolved="$(podman image inspect --format '{{.Id}}' "$IMAGE")"
  fi
  case "$resolved" in
    sha256:*) printf '%s\n' "$resolved" ;;
    *) printf 'sha256:%s\n' "$resolved" ;;
  esac
}

request_field() {
  python3 - "$1" "$2" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    value = json.load(handle)
for part in sys.argv[2].split("."):
    value = value[part]
print(value)
PY
}

merge_request() {
  python3 - "$1" "$2" "$3" "$4" "$5" <<'PY'
import json, sys
source, workspace, run_dir, extra, destination = sys.argv[1:6]
with open(source, encoding="utf-8") as handle:
    request = json.load(handle)
request["workspace"] = workspace
request["runDir"] = run_dir
if extra != "-":
    with open(extra, encoding="utf-8") as handle:
        request["check"] = json.load(handle)
with open(destination, "w", encoding="utf-8") as handle:
    json.dump(request, handle, sort_keys=True)
PY
}

run_container() {
  local phase="$1" request="$2" run_dir="$3" network="$4" timeout_secs="$5"
  local cpu="$6" memory="$7" pids="$8" workspace="$9"
  local digest
  digest="$(resolve_digest)" || typed_failure ENVIRONMENT_BUILDER_UNAVAILABLE \
    "builder image ${IMAGE} is not present or has no digest"

  local -a proxy_env=()
  if [[ -n "$FETCH_PROXY" ]]; then
    if [[ "$FETCH_PROXY" == *"@"* || ! "$FETCH_PROXY" =~ ^https?:// ]]; then
      typed_failure ENVIRONMENT_NETWORK_DENIED "FETCH_PROXY must be a credential-free HTTP(S) proxy"
    fi
    proxy_env=(
      -e "HTTP_PROXY=$FETCH_PROXY"
      -e "HTTPS_PROXY=$FETCH_PROXY"
      -e "ALL_PROXY=$FETCH_PROXY"
      -e "NO_PROXY=127.0.0.1,localhost"
      -e "npm_config_proxy=$FETCH_PROXY"
      -e "npm_config_https_proxy=$FETCH_PROXY"
    )
  fi

  local -a command=(
    podman run --rm -i
    --network "$network"
    --cap-drop ALL
    --cap-add SYS_ADMIN
    --cap-add SYS_CHROOT
    --security-opt unmask=ALL
    --security-opt no-new-privileges
    --user 10001:10001
    --userns=keep-id:uid=10001,gid=10001
    --cpus "$cpu"
    --memory "${memory}m"
    --pids-limit "$pids"
    --tmpfs /tmp:rw,noexec,nosuid,size=1g
    --mount "type=bind,src=${workspace},dst=/inputs/workspace,ro=true,Z"
    --mount "type=bind,src=${run_dir},dst=/cache,rw=true,Z"
    --env HOME=/home/factory
    --env "FACTORY_ENVIRONMENT_BUILDER_IMAGE=${IMAGE}"
    --env "FACTORY_ENVIRONMENT_BUILDER_DIGEST=${digest}"
    --env "FACTORY_ENVIRONMENT_BUILDER_PLATFORM=${PLATFORM}"
    --env "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION=${POLICY_REVISION}"
    --env "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID=$(basename "$run_dir")"
    "${proxy_env[@]}"
    "$IMAGE"
    "$phase"
    --request "/cache/request.json"
    --workspace /inputs/workspace
    --output /cache
  )
  timeout --signal=TERM --kill-after=10s "$((timeout_secs + 60))" "${command[@]}"
}

copy_outputs() {
  local run_dir="$1" output="$2"
  mkdir -p "$output"
  local name
  for name in result.json builder-evidence.json build.log check.log check-result.json \
              fetch-result.json sbom.cdx.json license-inventory.json closure.json; do
    if [[ -f "$run_dir/$name" ]]; then
      cp -f "$run_dir/$name" "$output/$name"
    fi
  done
}

prepare() {
  local request="$1" workspace="$2" run_dir="$3" check="$4"
  mkdir -p "$run_dir"
  merge_request "$request" /inputs/workspace /cache "$check" "$run_dir/request.json"
  # The project is read-only to the builder. Nix/devenv writes only into the
  # ephemeral run workspace inside the cache mount.
  python3 - "$run_dir/request.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    request = json.load(handle)
resources = request.get("plan", {}).get("resources", {})
for key, default in (("cpu", 2), ("memoryMiB", 4096), ("pids", 512), ("timeoutSecs", 1800)):
    value = resources.get(key, default)
    if not isinstance(value, int) or value <= 0:
        raise SystemExit(f"invalid resource {key}")
print(f"{resources.get('cpu', 2)} {resources.get('memoryMiB', 4096)} {resources.get('pids', 512)} {resources.get('timeoutSecs', 1800)}")
PY
}

cmd="${1:-}"
case "$cmd" in
  descriptor)
    digest="$(resolve_digest)" || typed_failure ENVIRONMENT_BUILDER_UNAVAILABLE \
      "builder image ${IMAGE} is not present or has no digest"
    podman run --rm --network none --cap-drop ALL --security-opt unmask=ALL \
      --security-opt no-new-privileges --user 10001:10001 --userns=keep-id:uid=10001,gid=10001 \
      -e "FACTORY_ENVIRONMENT_BUILDER_IMAGE=${IMAGE}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_DIGEST=${digest}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_PLATFORM=${PLATFORM}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION=${POLICY_REVISION}" \
      "$IMAGE" descriptor
    ;;
  build|fetch|check)
    shift
    request=""
    workspace=""
    output=""
    check_doc="-"
    fetch_first="0"
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --request) request="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        --workspace) workspace="$(cd "$2" && pwd)"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --check) check_doc="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        --fetch) fetch_first="1"; shift ;;
        *) log "unknown argument: $1"; exit 2 ;;
      esac
    done
    [[ -n "$request" && -f "$request" ]] || typed_failure ENVIRONMENT_SPEC_INVALID "a request JSON file is required"
    [[ -n "$workspace" && -d "$workspace" ]] || typed_failure ENVIRONMENT_PLAN_INVALID "a project workspace is required"
    [[ -n "$output" ]] || typed_failure ENVIRONMENT_SPEC_INVALID "--output is required"
    mkdir -p "$CACHE"
    run_id="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
    run_dir="$CACHE/runs/$run_id"
    read -r cpu memory pids timeout_secs < <(prepare "$request" "$workspace" "$run_dir" "$check_doc")

    if [[ "$cmd" == "build" && "$fetch_first" == "1" ]]; then
      if ! run_container fetch /cache/request.json "$run_dir" "$FETCH_NETWORK" "$timeout_secs" "$cpu" "$memory" "$pids" "$workspace"; then
        copy_outputs "$run_dir" "$output"
        typed_failure ENVIRONMENT_BUILD_FAILED "the broker fetch phase failed"
      fi
    fi

    network="none"
    if [[ "$cmd" == "fetch" || ( "$cmd" == "build" && "$fetch_first" == "1" ) ]]; then
      network="$FETCH_NETWORK"
    fi
    phase="$cmd"
    if ! run_container "$phase" /cache/request.json "$run_dir" "$network" "$timeout_secs" "$cpu" "$memory" "$pids" "$workspace"; then
      copy_outputs "$run_dir" "$output"
      typed_failure ENVIRONMENT_BUILD_FAILED "the builder container failed; see build.log/result.json"
    fi
    # The requested check document is merged into /cache/request.json; the
    # entrypoint writes its own check result there.
    copy_outputs "$run_dir" "$output"
    if [[ -f "$output/result.json" ]]; then
      cat "$output/result.json"
    elif [[ -f "$output/check-result.json" ]]; then
      cat "$output/check-result.json"
    fi
    ;;
  *)
    sed -n '2,25p' "$0" >&2
    exit 2
    ;;
esac
