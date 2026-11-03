#!/usr/bin/env bash
# Reproducible comparison driver.
#
# It measures the isolated Infinity prototype and the existing Factory
# deterministic fixture dispatch test as real process trees. The Factory test
# includes Core/Supervisor/Worker/Verifier/Reviewer governance that the
# prototype does not, so the results are intentionally reported as
# "system-level" and not as an agent-only microbenchmark.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
EXP="$REPO/infinity-experiment"
OUT="${1:-$REPO/infinity-experiment/evidence/comparison-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT"

PROFILE="${PROFILE:-debug}"
if [[ "$PROFILE" == "release" ]]; then
  EXP_BIN="$EXP/target/release/infinity-experiment"
  (cd "$EXP" && cargo build --release --bin infinity-experiment)
else
  EXP_BIN="$EXP/target/debug/infinity-experiment"
  (cd "$EXP" && cargo build --bin infinity-experiment)
fi

for run in 1 2 3 4 5; do
  ROOT="$OUT/prototype-$run"
  rm -rf "$ROOT"
  python3 "$HERE/measure.py" \
    --label "prototype-$run" \
    --out "$OUT/prototype-$run.json" \
    -- "$EXP_BIN" run --root "$ROOT" --run-id "measure-$run" \
       --effect-delay-ms 10 --out "$ROOT/report.json" >/dev/null
done

# Locate the already-built Factory integration test binary.
TEST_BIN="$(cd "$REPO" && cargo test -p factory-native --test e2e_fixture_dispatch --no-run --message-format=json 2>/dev/null \
  | python3 -c 'import json,sys
for line in sys.stdin:
    try: item=json.loads(line)
    except Exception: continue
    if item.get("reason")=="compiler-artifact" and item.get("target",{}).get("name")=="e2e_fixture_dispatch":
        print(item["executable"]); break')"
if [[ -n "$TEST_BIN" ]]; then
  python3 "$HERE/measure.py" \
    --label "factory-parallel-workgraph" \
    --out "$OUT/factory-parallel-workgraph.json" \
    -- "$TEST_BIN" parallel_workgraph_reaches_accepted_on_integrated_candidate --nocapture \
    >/dev/null || true
else
  echo "warning: Factory baseline test binary not found" >&2
fi

RECOVERY_BIN="$(cd "$REPO" && cargo test -p factory-native --test e2e_core_recovery --no-run --message-format=json 2>/dev/null \
  | python3 -c 'import json,sys
for line in sys.stdin:
    try: item=json.loads(line)
    except Exception: continue
    if item.get("reason")=="compiler-artifact" and item.get("target",{}).get("name")=="e2e_core_recovery":
        print(item["executable"]); break')"
if [[ -n "$RECOVERY_BIN" ]]; then
  python3 "$HERE/measure.py" \
    --label "factory-core-recovery-e2e" \
    --out "$OUT/factory-core-recovery.json" \
    -- "$RECOVERY_BIN" core_restart_preserves_durable_project_action_and_events --nocapture \
    >/dev/null || true
fi

# Prototype process-restart journey: start, crash at the review marker, then
# recover in a new process. Measured as one shell workload.
RESTART_ROOT="$OUT/prototype-restart"
rm -rf "$RESTART_ROOT"
python3 "$HERE/measure.py" \
  --label "prototype-restart-recovery" \
  --out "$OUT/prototype-restart-recovery.json" \
  -- bash -c "'$EXP_BIN' start --root '$RESTART_ROOT' --run-id restart --crash-after-effect review --effect-delay-ms 10 >/dev/null 2>&1; code=\$?; test \$code -eq 23 && '$EXP_BIN' resume --root '$RESTART_ROOT' --run-id restart --effect-delay-ms 10 --out '$RESTART_ROOT/report.json' >/dev/null" \
  >/dev/null || true

python3 "$HERE/summarize.py" "$OUT"
