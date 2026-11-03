#!/usr/bin/env bash
# Produce a compact, reproducible restart-recovery evidence file.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
EXP="$(cd "$HERE/.." && pwd)"
BIN="$EXP/target/debug/infinity-experiment"
OUT="${1:-$EXP/evidence/restart-evidence.json}"
ROOT="$(mktemp -d /tmp/factory-infinity-restart-XXXXXX)"
trap 'rm -rf "$ROOT"' EXIT

set +e
"$BIN" start --root "$ROOT" --run-id evidence \
  --crash-after-effect review --effect-delay-ms 10 >/dev/null 2>&1
START_CODE=$?
set -e
if [[ "$START_CODE" -ne 23 ]]; then
  echo "expected crash exit 23, got $START_CODE" >&2
  exit 1
fi

EFFECTS_BEFORE="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from effects;")"
APPLIED_BEFORE="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from effects where state='APPLIED';")"
CALLBACKS_BEFORE="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from callbacks;")"
APPLICATIONS_BEFORE="$(wc -l < "$ROOT/workspace/applied-effects.log" 2>/dev/null || echo 0)"

"$BIN" resume --root "$ROOT" --run-id evidence \
  --effect-delay-ms 10 --out "$ROOT/report.json" >/dev/null

APPLICATIONS_AFTER="$(wc -l < "$ROOT/workspace/applied-effects.log")"
EFFECTS_AFTER="$(sqlite3 "$ROOT/factory.sqlite" "select count(*) from effects;")"
COMPLETED_AFTER="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from effects where state='COMPLETED';")"
AMBIGUOUS_AFTER="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from effects where state='AMBIGUOUS';")"
CALLBACKS_AFTER="$(sqlite3 "$ROOT/factory.sqlite" "select count(*) from callbacks;")"
DELIVERED_AFTER="$(sqlite3 "$ROOT/factory.sqlite" \
  "select count(*) from callbacks where status='DELIVERED';")"

python3 - "$OUT" "$ROOT/report.json" "$START_CODE" \
  "$EFFECTS_BEFORE" "$APPLIED_BEFORE" "$CALLBACKS_BEFORE" "$APPLICATIONS_BEFORE" \
  "$EFFECTS_AFTER" "$COMPLETED_AFTER" "$AMBIGUOUS_AFTER" "$CALLBACKS_AFTER" \
  "$DELIVERED_AFTER" "$APPLICATIONS_AFTER" <<'PY'
import json
import sys
from pathlib import Path

(
    out,
    report_path,
    start_code,
    effects_before,
    applied_before,
    callbacks_before,
    applications_before,
    effects_after,
    completed_after,
    ambiguous_after,
    callbacks_after,
    delivered_after,
    applications_after,
) = sys.argv[1:]
report = json.loads(Path(report_path).read_text())
data = {
    "crash_exit_code": int(start_code),
    "before_resume": {
        "effects": int(effects_before),
        "applied_not_completed": int(applied_before),
        "callbacks": int(callbacks_before),
        "external_applications": int(applications_before),
    },
    "after_resume": {
        "effects": int(effects_after),
        "completed": int(completed_after),
        "ambiguous": int(ambiguous_after),
        "callbacks": int(callbacks_after),
        "delivered": int(delivered_after),
        "external_applications": int(applications_after),
        "duplicate_applications": int(applications_after) - int(applications_before)
        - (int(effects_after) - int(effects_before)),
    },
    "report": {
        "success": report.get("success"),
        "candidate_sha256": (report.get("candidate") or {}).get("candidate_sha256"),
        "recovery": report.get("recovery"),
        "metrics": {
            "wall_ms": report.get("metrics", {}).get("wall_ms"),
            "restart_recovery_ms": report.get("metrics", {}).get(
                "restart_recovery_ms"
            ),
            "duplicate_external_effects": report.get("metrics", {}).get(
                "duplicate_external_effects"
            ),
        },
    },
}
Path(out).parent.mkdir(parents=True, exist_ok=True)
Path(out).write_text(json.dumps(data, indent=2) + "\n")
print(json.dumps(data, indent=2))
PY
