#!/usr/bin/env bash
# Rootless Podman wrapper for the generic Nix/devenv environment builder.
#
# This is a development/CI harness. The installed product must invoke the same
# image through the runtime broker in the canonical Compose topology; it must
# not gain a host toolchain, a host socket or a host bind beyond the explicit
# read-only project and writable cache mount below.
#
# Fetch egress is deny-by-default. The only accepted networks are `none` and the
# reviewed broker networks (`factory-egress` / `factory-deps-egress`, including
# the Compose-prefixed form). `host`, `container`, `slirp4netns`, `pasta` and
# arbitrary networks are rejected. A non-`none` network requires a
# credential-free HTTP(S) proxy; the plan allowlist is passed to the broker and
# the builder records it. Without a proxy the fetch phase fails closed.
#
# Usage:
#   scripts/environment-builder.sh build --request REQUEST.json --workspace DIR --output DIR [--fetch]
#   scripts/environment-builder.sh fetch --request REQUEST.json --workspace DIR --output DIR
#   scripts/environment-builder.sh check --request CHECK.json --workspace DIR --output DIR [--run-dir DIR]
#   scripts/environment-builder.sh descriptor
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
DIGEST="${FACTORY_ENVIRONMENT_BUILDER_DIGEST:-}"
PLATFORM="${FACTORY_ENVIRONMENT_BUILDER_PLATFORM:-x86_64-linux}"
POLICY_REVISION="${FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION:-nix-devenv-environment-v1}"
CACHE="${FACTORY_ENVIRONMENT_BUILDER_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/software-factory/environment-builder}"
CACHE="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$CACHE")"
NIX_STORE_CACHE="${FACTORY_ENVIRONMENT_BUILDER_NIX_STORE:-$CACHE/nix-store}"
NIX_STORE_CACHE="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$NIX_STORE_CACHE")"
FETCH_NETWORK="${FACTORY_ENVIRONMENT_BUILDER_FETCH_NETWORK:-none}"
FETCH_PROXY="${FACTORY_ENVIRONMENT_BUILDER_FETCH_PROXY:-}"
APPROVED_FETCH_NETWORKS=(
  none
  factory-egress-internal
  factory-deps-egress
  software-factory_factory-egress-internal
  software-factory_factory-deps-egress
)

RUN_REF=""
VERIFIED_DIGEST=""
VERIFIED_REPO_DIGEST=""

log() { printf '%s\n' "$*" >&2; }

canonical() {
  python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"
}

require_within_cache() {
  local label="$1" path="$2"
  local real
  real="$(canonical "$path")"
  case "$real" in
    "${CACHE}"/*) printf '%s\n' "$real" ;;
    *) typed_failure ENVIRONMENT_RESOURCE_LIMIT \
         "${label} must stay inside the builder cache root ${CACHE}" ;;
  esac
}

validate_cache_root() {
  local cache="$1"
  python3 - "$cache" "$HOME" "$ROOT" <<'PYCACHE'
import os, sys
cache, home, root = sys.argv[1:4]
real = os.path.realpath(cache)
forbidden = {"/", "/root", os.path.realpath(home), os.path.realpath(root)}
if real in forbidden:
    raise SystemExit(f"cache root {real} is a protected host path")
print(real)
PYCACHE
}

validate_workspace() {
  local workspace="$1"
  python3 - "$workspace" "$HOME" "$ROOT" <<'PYWORKSPACE'
import os, sys
workspace, home, root = sys.argv[1:4]
real = os.path.realpath(workspace)
forbidden = {
    "/", "/root", "/home", "/etc", "/proc", "/sys", "/dev", "/run", "/var/run",
    os.path.realpath(home), os.path.realpath(root),
}
if real in forbidden:
    raise SystemExit(f"workspace {real} is a protected host path and must not be mounted")
if not os.path.isdir(real):
    raise SystemExit(f"workspace {real} is not a directory")
if "/.local/share/containers" in real or "podman.sock" in real:
    raise SystemExit("workspace must not expose the container engine")
print(real)
PYWORKSPACE
}

disk_usage_kib() {
  local run_dir="$1" store_dir="$2"
  du -sk "$run_dir" "$store_dir" 2>/dev/null | awk '{sum += $1} END {print sum + 0}'
}

check_disk_budget() {
  local run_dir="$1" store_dir="$2" disk_mib="$3" phase="$4"
  local observed
  observed="$(disk_usage_kib "$run_dir" "$store_dir")"
  python3 - "$run_dir" "$store_dir" "$disk_mib" "$observed" "$phase" <<'PYDISK'
import json, os, sys
run_dir, store_dir, disk_mib, observed, phase = sys.argv[1:6]
plan = int(disk_mib)
observed = int(observed)
record = {
    "schemaVersion": 1,
    "phase": phase,
    "planDiskMiB": plan,
    "observedKiB": observed,
    "observedMiB": round(observed / 1024, 3),
    "enforcement": "POST_HOC_DU",
    "exact": False,
    "exceeded": observed > plan * 1024,
    "runDir": run_dir,
    "storeCache": store_dir,
}
with open(os.path.join(run_dir, "disk-usage.json"), "w", encoding="utf-8") as handle:
    json.dump(record, handle, indent=2, sort_keys=True)
    handle.write("\n")
if record["exceeded"]:
    raise SystemExit(1)
PYDISK
}

typed_failure() {
  python3 - "$1" "$2" <<'PY'
import json, sys
from pathlib import Path
print(json.dumps({"status": "FAILED", "blocker": {"code": sys.argv[1], "message": sys.argv[2]}}, sort_keys=True))
PY
  exit 2
}

validate_fetch_network() {
  local value="$1"
  case "$value" in
    host|container|slirp4netns|pasta|bridge|""|*/*|*' '*)
      typed_failure ENVIRONMENT_NETWORK_DENIED \
        "fetch network '${value}' is not an approved broker network; host/container/arbitrary networks are forbidden"
      ;;
  esac
  local approved
  for approved in "${APPROVED_FETCH_NETWORKS[@]}"; do
    if [[ "$value" == "$approved" ]]; then
      if [[ "$value" == "none" ]]; then
        return 0
      fi
      local internal
      if ! internal="$(podman network inspect "$value" --format '{{.Internal}}' 2>/dev/null)"; then
        typed_failure ENVIRONMENT_NETWORK_DENIED \
          "fetch network '${value}' cannot be inspected; fail closed"
      fi
      if [[ "$internal" != "true" ]]; then
        typed_failure ENVIRONMENT_NETWORK_DENIED \
          "fetch network '${value}' is not internal; direct egress bypass is forbidden"
      fi
      return 0
    fi
  done
  typed_failure ENVIRONMENT_NETWORK_DENIED \
    "fetch network '${value}' is not an approved internal broker network"
}

validate_fetch_proxy() {
  local network="$1" proxy="$2"
  if [[ "$network" == "none" ]]; then
    return 0
  fi
  if [[ -z "$proxy" ]]; then
    typed_failure ENVIRONMENT_NETWORK_DENIED \
      "fetch network ${network} requires FACTORY_ENVIRONMENT_BUILDER_FETCH_PROXY: the allowlist cannot be enforced without the approved broker proxy"
  fi
  if [[ "$proxy" == *"@"* || ! "$proxy" =~ ^https?:// ]]; then
    typed_failure ENVIRONMENT_NETWORK_DENIED "fetch proxy must be a credential-free HTTP(S) proxy"
  fi
}

# Inspect the image and bind the supplied digest to an actual image identity.
# The tag is never used for execution: a local image runs by its image ID and a
# registry image runs by image@manifest-digest.
verify_image() {
  if ! podman image exists "$IMAGE" >/dev/null 2>&1; then
    typed_failure ENVIRONMENT_BUILDER_UNAVAILABLE \
      "builder image ${IMAGE} is not present"
  fi
  local image_id repo_digest
  image_id="$(podman image inspect --format '{{.Id}}' "$IMAGE")"
  repo_digest="$(podman image inspect --format '{{.Digest}}' "$IMAGE" 2>/dev/null || true)"
  case "$image_id" in sha256:*) ;; *) image_id="sha256:${image_id}" ;; esac
  if [[ -n "$DIGEST" ]]; then
    if [[ "$DIGEST" != "$image_id" && "$DIGEST" != "$repo_digest" ]]; then
      typed_failure ENVIRONMENT_HASH_MISMATCH \
        "configured builder digest ${DIGEST} does not match the inspected image ${image_id} ${repo_digest}"
    fi
    VERIFIED_DIGEST="$DIGEST"
  elif [[ -n "$repo_digest" ]]; then
    VERIFIED_DIGEST="$repo_digest"
  else
    VERIFIED_DIGEST="$image_id"
  fi
  VERIFIED_REPO_DIGEST="$repo_digest"
  if [[ "$VERIFIED_DIGEST" == "$repo_digest" && -n "$repo_digest" ]]; then
    RUN_REF="${IMAGE}@${repo_digest}"
  else
    RUN_REF="$image_id"
  fi
  if ! podman image inspect --format '{{.Id}}' "$RUN_REF" >/dev/null 2>&1; then
    typed_failure ENVIRONMENT_BUILDER_UNAVAILABLE \
      "builder image reference ${RUN_REF} could not be resolved by digest"
  fi
}

# A persistent store cache is a cache mount, not a host runtime dependency. It
# lets the fetch and build phases share the exact fetched derivations/sources
# without exporting/importing the whole store through a NAR. It is seeded once
# from the verified image and remains under the cache directory.
ensure_nix_store() {
  mkdir -p "$NIX_STORE_CACHE"
  if [[ -e "$NIX_STORE_CACHE/.seeded" ]]; then
    return 0
  fi
  podman run --rm --entrypoint sh --user 10001:10001 \
    --userns=keep-id:uid=10001,gid=10001 \
    -v "${NIX_STORE_CACHE}:/seed:Z" \
    "$RUN_REF" -c 'cp -r /nix/. /seed/ && touch /seed/.seeded'
}

merge_request() {
  python3 - "$1" "$2" "$3" "$4" "$5" "$6" "$7" <<'PY'
import json, sys
from pathlib import Path
source, workspace, run_dir, extra, destination, network, proxy = sys.argv[1:8]
with open(source, encoding="utf-8") as handle:
    request = json.load(handle)
request["workspace"] = workspace
request["runDir"] = run_dir
request["fetchNetwork"] = network
if proxy:
    request["fetchProxy"] = proxy
plan = request.get("plan", {})
allowlist = plan.get("network", {}).get("fetchAllowlist", [])
request["fetchPolicy"] = {
    "network": network,
    "proxy": proxy or None,
    "allowlist": allowlist,
    "enforcedBy": "broker" if network != "none" else "offline",
}
receipt_path = Path(run_dir) / "result.json"
if receipt_path.is_file():
    request["receipt"] = json.loads(receipt_path.read_text(encoding="utf-8"))
if extra != "-":
    with open(extra, encoding="utf-8") as handle:
        request["check"] = json.load(handle)
with open(destination, "w", encoding="utf-8") as handle:
    json.dump(request, handle, sort_keys=True)
PY
}

prepare() {
  local request="$1" workspace="$2" run_dir="$3" check="$4" network="$5" proxy="$6"
  mkdir -p "$run_dir"
  merge_request "$request" /inputs/workspace "$run_dir" "$check" "$run_dir/request.json" "$network" "$proxy"
  python3 - "$run_dir/request.json" <<'PY'
import json, sys
from pathlib import Path
with open(sys.argv[1], encoding="utf-8") as handle:
    request = json.load(handle)
plan_resources = request.get("plan", {}).get("resources", {})
spec_resources = request.get("spec", {}).get("resourcePolicy", {})
if plan_resources != spec_resources:
    raise SystemExit("plan resources do not match the frozen spec resource policy")
for key, default in (("cpu", 2), ("memoryMiB", 4096), ("pids", 512), ("timeoutSecs", 1800), ("diskMiB", 10240)):
    value = plan_resources.get(key, default)
    if not isinstance(value, int) or value <= 0:
        raise SystemExit(f"invalid resource {key}")
print(f"{plan_resources.get('cpu', 2)} {plan_resources.get('memoryMiB', 4096)} {plan_resources.get('pids', 512)} {plan_resources.get('timeoutSecs', 1800)} {plan_resources.get('diskMiB', 10240)}")
PY
}

run_container() {
  local phase="$1" run_dir="$2" network="$3" timeout_secs="$4"
  local cpu="$5" memory="$6" pids="$7" workspace="$8"

  local -a proxy_env=()
  if [[ -n "$FETCH_PROXY" && "$network" != "none" ]]; then
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
    --mount "type=bind,src=${workspace},dst=/inputs/workspace,ro=true"
    --mount "type=bind,src=${run_dir},dst=/cache,rw=true"
    --mount "type=bind,src=${NIX_STORE_CACHE},dst=/nix,rw=true"
    --env HOME=/home/factory
    --env FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE=1
    --env "FACTORY_ENVIRONMENT_BUILDER_IMAGE=${IMAGE}"
    --env "FACTORY_ENVIRONMENT_BUILDER_DIGEST=${VERIFIED_DIGEST}"
    --env "FACTORY_ENVIRONMENT_BUILDER_PLATFORM=${PLATFORM}"
    --env "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION=${POLICY_REVISION}"
    --env "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID=$(basename "$run_dir")"
    "${proxy_env[@]}"
    "$RUN_REF"
    "$phase"
    --request "/cache/request.json"
    --workspace /inputs/workspace
    --output /cache
  )
  timeout --signal=TERM --kill-after=5s "$timeout_secs" "${command[@]}"
}

copy_outputs() {
  local run_dir="$1" output="$2"
  mkdir -p "$output"
  local name
  for name in result.json builder-evidence.json build.log check.log check-result.json \
              fetch-result.json sbom.cdx.json license-inventory.json closure.json sbom-digest.json disk-usage.json; do
    if [[ -f "$run_dir/$name" ]]; then
      cp -f "$run_dir/$name" "$output/$name"
    fi
  done
}

cmd="${1:-}"
case "$cmd" in
  descriptor)
    verify_image
    podman run --rm --network none --cap-drop ALL --security-opt unmask=ALL \
      --security-opt no-new-privileges --user 10001:10001 \
      --userns=keep-id:uid=10001,gid=10001 \
      -e "FACTORY_ENVIRONMENT_BUILDER_IMAGE=${IMAGE}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_DIGEST=${VERIFIED_DIGEST}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_PLATFORM=${PLATFORM}" \
      -e "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION=${POLICY_REVISION}" \
      "$RUN_REF" descriptor
    ;;
  build|fetch|check)
    shift
    request=""
    workspace=""
    output=""
    check_doc="-"
    fetch_first="0"
    run_dir_override=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --request) request="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        --workspace) workspace="$(cd "$2" && pwd)"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --check) check_doc="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        --run-dir) run_dir_override="$2"; shift 2 ;;
        --fetch) fetch_first="1"; shift ;;
        --fetch-network) FETCH_NETWORK="$2"; shift 2 ;;
        --fetch-proxy) FETCH_PROXY="$2"; shift 2 ;;
        *) log "unknown argument: $1"; exit 2 ;;
      esac
    done
    [[ -n "$request" && -f "$request" ]] || typed_failure ENVIRONMENT_SPEC_INVALID "a request JSON file is required"
    [[ -n "$workspace" && -d "$workspace" ]] || typed_failure ENVIRONMENT_PLAN_INVALID "a project workspace is required"
    [[ -n "$output" ]] || typed_failure ENVIRONMENT_SPEC_INVALID "--output is required"
    workspace="$(validate_workspace "$workspace")" || typed_failure ENVIRONMENT_PLAN_INVALID "workspace is not an allowed project path"
    validate_fetch_network "$FETCH_NETWORK"
    validate_fetch_proxy "$FETCH_NETWORK" "$FETCH_PROXY"
    verify_image
    if ! CACHE="$(validate_cache_root "$CACHE")"; then
      typed_failure ENVIRONMENT_RESOURCE_LIMIT "builder cache root is a protected host path"
    fi
    mkdir -p "$CACHE"
    chmod 700 "$CACHE" 2>/dev/null || true
    if ! NIX_STORE_CACHE="$(require_within_cache "nix store cache" "$NIX_STORE_CACHE")"; then
      typed_failure ENVIRONMENT_RESOURCE_LIMIT "nix store cache must stay inside the builder cache root ${CACHE}"
    fi
    ensure_nix_store
    if [[ -n "$run_dir_override" ]]; then
      if ! run_dir="$(require_within_cache "run directory" "$run_dir_override")"; then
        typed_failure ENVIRONMENT_RESOURCE_LIMIT "run directory must stay inside the builder cache root ${CACHE}"
      fi
      mkdir -p "$run_dir"
    else
      run_id="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
      run_dir="$CACHE/runs/$run_id"
    fi
    read -r cpu memory pids timeout_secs disk_mib < <(prepare "$request" "$workspace" "$run_dir" "$check_doc" "$FETCH_NETWORK" "$FETCH_PROXY")

    if ! check_disk_budget "$run_dir" "$NIX_STORE_CACHE" "$disk_mib" "pre-${cmd}"; then
      copy_outputs "$run_dir" "$output" 2>/dev/null || true
      typed_failure ENVIRONMENT_RESOURCE_LIMIT "builder cache/store already exceeds the frozen disk bound"
    fi
    if [[ "$cmd" == "build" && "$fetch_first" == "1" ]]; then
      if ! run_container fetch "$run_dir" "$FETCH_NETWORK" "$timeout_secs" "$cpu" "$memory" "$pids" "$workspace"; then
        copy_outputs "$run_dir" "$output"
        typed_failure ENVIRONMENT_BUILD_FAILED "the broker fetch phase failed"
      fi
      if [[ ! -f "$run_dir/fetch-result.json" ]]; then
        copy_outputs "$run_dir" "$output"
        typed_failure ENVIRONMENT_BUILD_FAILED "the broker fetch phase produced no fetch result"
      fi
    fi

    network="none"
    if [[ "$cmd" == "fetch" ]]; then
      network="$FETCH_NETWORK"
    fi
    if [[ "$cmd" == "build" ]]; then
      python3 - "$run_dir/request.json" <<'PYREQUEST'
import json, sys
path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    request = json.load(handle)
request["fetchNetwork"] = "none"
request.pop("fetchProxy", None)
policy = request.get("fetchPolicy")
if isinstance(policy, dict):
    policy["network"] = "none"
    policy["proxy"] = None
    policy["enforcedBy"] = "offline"
with open(path, "w", encoding="utf-8") as handle:
    json.dump(request, handle, sort_keys=True)
PYREQUEST
    fi
    if ! run_container "$cmd" "$run_dir" "$network" "$timeout_secs" "$cpu" "$memory" "$pids" "$workspace"; then
      copy_outputs "$run_dir" "$output"
      typed_failure ENVIRONMENT_BUILD_FAILED "the builder container failed; see build.log/result.json"
    fi
    if ! check_disk_budget "$run_dir" "$NIX_STORE_CACHE" "$disk_mib" "post-${cmd}"; then
      rm -f "$run_dir/result.json"
      copy_outputs "$run_dir" "$output"
      typed_failure ENVIRONMENT_RESOURCE_LIMIT "observed builder disk usage exceeds the frozen disk bound"
    fi
    copy_outputs "$run_dir" "$output"
    if [[ -f "$output/result.json" ]]; then
      cat "$output/result.json"
    elif [[ -f "$output/check-result.json" ]]; then
      cat "$output/check-result.json"
    fi
    ;;
  *)
    sed -n '2,26p' "$0" >&2
    exit 2
    ;;
esac
