#!/usr/bin/env bash
# Bounded Option-B feasibility probe for the identity-facts licence origin.
#
# It builds a disposable, own Nix store containing the exact reconstructed
# Run10 outputs, verifies their NAR bytes with a real `nix-store --dump`, extracts
# the exact Nix reference sets, and decides whether a truthful Nix validity
# registration is possible from bytes available offline.
#
# It never builds, fetches, writes to a retained volume, or fabricates a
# validity row. If a referenced path has no bytes and no validity row in the
# allowed offline sources (the pinned builder image and the retained Run10
# store, both read-only), it reports BLOCKED with the exact paths and Nix's own
# invalid-path error, and exits 0 (a factual blocked result).
#
# Required environment:
#   FACTORY_OPTIONB_EVIDENCE_DIR  composition-audit directory with stdenv/ and hook/
# Optional:
#   FACTORY_OPTIONB_RUN10_VOLUME  retained Run10 store volume (default run10-coverage-nix)
#   FACTORY_IDENTITY_FACTS_REAL_IMAGE (default localhost/software-factory-environment-builder:dev)
#   FACTORY_PODMAN_BIN (default podman)
set -euo pipefail

IMAGE=${FACTORY_IDENTITY_FACTS_REAL_IMAGE:-localhost/software-factory-environment-builder:dev}
PODMAN=${FACTORY_PODMAN_BIN:-$(command -v podman)}
EVIDENCE=${FACTORY_OPTIONB_EVIDENCE_DIR:?FACTORY_OPTIONB_EVIDENCE_DIR is required}
RUN10=${FACTORY_OPTIONB_RUN10_VOLUME:-run10-coverage-nix}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
WORK=${FACTORY_OPTIONB_WORK:-/tmp/opencode/identity-facts-optionb-$STAMP}
VOLUME=${FACTORY_OPTIONB_VOLUME:-factory-optionb-probe-$STAMP}
mkdir -p "$WORK"
chmod 700 "$WORK"

STDENV_PATH=/nix/store/lfwfj17y9fpjb73nsj2m35rmkh587a0x-stdenv-linux
STDENV_SRI=sha256-7uyA5WT1X+9o1QV/Dqs7GsmWtellurWH5ipGyZK8Szk=
HOOK_PATH=/nix/store/f244pq7qm9ga22jpa4l0vca2ys4q7wh7-update-autotools-gnu-config-scripts-hook
HOOK_SRI=sha256-wIte/PovG1/aVE3yV/TmEf9bQKvXYDVlNJB+wcOZP8w=
# The five references with no validity row and no bytes in the seeded closure or
# the retained store. Only these exact paths are checked in the full image store.
FIVE_PATHS=(
  /nix/store/4zpfjm2r6chc8b10c1xa1d6asy100sqv-file-5.45
  /nix/store/5wf9wpdkxs30811kfgkicn9i3nz9jhsh-patchelf-0.15.0
  /nix/store/jangxddrzycnc2rl1wc1sqhqp26yrx4k-patch-2.7.6
  /nix/store/xpsgmdsqym0ydx6hkvpxrp6174s4fwbl-gnumake-4.4.1
  /nix/store/rwyj9q50lxgg2krci51hr7afiql2p8km-gnu-config-2024-01-01
)

KEEP=${FACTORY_OPTIONB_KEEP:-0}
cleanup() {
  if [ "$KEEP" = "1" ]; then
    echo "KEEP=1; leaving volume $VOLUME" >&2
    return 0
  fi
  "$PODMAN" volume rm --force "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "work=$WORK"
echo "volume=$VOLUME"
echo "image=$IMAGE"
echo "run10_volume=$RUN10"
echo "evidence=$EVIDENCE"

"$PODMAN" volume create "$VOLUME" >/dev/null 2>&1 || true

# 1. Seed the disposable store with the interpreter/Nix closure and the image
#    validity database. This mirrors the reviewed identity-facts seeding and
#    never writes to the image or the retained volume.
"$PODMAN" run --rm --network=none --user=0:0 \
  -v "$VOLUME:/target:rw" --entrypoint=/bin/sh "$IMAGE" -c '
set -eu
root=$(readlink -f /bin/sh); root=${root%/bin/*}
nixpath=$(readlink -f "$(command -v nix)"); nixroot=${nixpath%/bin/*}
readlinkpath=$(readlink -f "$(command -v readlink)"); readlinkroot=${readlinkpath%/bin/*}
paths=$(nix path-info --json --recursive --offline "$root" "$nixroot" "$readlinkroot" | jq -r "keys[]")
drvs=$(for p in $paths; do d=$(nix-store -q --deriver "$p" 2>/dev/null || true); if [ -n "$d" ] && [ "$d" != unknown-deriver ]; then printf "%s\n" "$d"; fi; done | sort -u)
tar -C /nix -cf - $(for p in $paths $drvs; do printf "%s\n" "${p#/nix/}"; done) var/nix/db | tar -C /target -xf -
mkdir -p /target/var/nix/profiles/default/bin
ln -sfn "$nixroot/bin/nix" /target/var/nix/profiles/default/bin/nix
ln -sfn "$nixroot/bin/nix-store" /target/var/nix/profiles/default/bin/nix-store
ln -sfn "$readlinkroot/bin/readlink" /target/var/nix/profiles/default/bin/readlink
'

# 2. Place the exact reconstructed output trees and derivations in the
#    disposable store. The derivation bytes come from the same pinned image;
#    the trees come from the frozen composition audit.
"$PODMAN" run --rm --network=none --user=0:0 \
  -v "$VOLUME:/target:rw" -v "$EVIDENCE:/ev:ro" \
  --entrypoint=/bin/sh "$IMAGE" -c '
set -eu
mkdir -p /target/store
cp -a /ev/stdenv/out /target/store/lfwfj17y9fpjb73nsj2m35rmkh587a0x-stdenv-linux
cp -a /ev/hook/out /target/store/f244pq7qm9ga22jpa4l0vca2ys4q7wh7-update-autotools-gnu-config-scripts-hook
cp /nix/store/nsvrknrqxdf2lxcfbqkr0p05897ww5lx-stdenv-linux.drv /target/store/ || true
cp /nix/store/n4jk7pj33qywkrlsgnf5611lc1c7wp08-update-autotools-gnu-config-scripts-hook.drv /target/store/ || true
'

# 3. Verify the reconstructed NAR bytes with the real Nix serializer. The NAR
#    stream is hashed on the host so only the real serializer runs in the image.
dump_hash() {
  "$PODMAN" run --rm --network=none --read-only --entrypoint=/bin/sh \
    -v "$VOLUME:/nix:ro" "$IMAGE" -c "nix-store --dump '$1'" 2>/dev/null \
    | sha256sum | cut -d' ' -f1
}
STDENV_HEX=$(dump_hash "$STDENV_PATH")
HOOK_HEX=$(dump_hash "$HOOK_PATH")
echo "stdenv_nar_sha256=$STDENV_HEX"
echo "hook_nar_sha256=$HOOK_HEX"

# 4. Extract the exact reference sets from the reconstructed bytes, as Nix
#    reference scanning does (32-character store hashes).
grep -oE '/nix/store/[a-z0-9]{32}-[A-Za-z0-9._+?=-]+' \
  "$EVIDENCE/stdenv/out/setup" | sort -u >"$WORK/stdenv-refs.txt"
grep -oE '/nix/store/[a-z0-9]{32}-[A-Za-z0-9._+?=-]+' \
  "$EVIDENCE/hook/out/nix-support/setup-hook" | sort -u >"$WORK/hook-refs.txt"
cat "$WORK/stdenv-refs.txt" "$WORK/hook-refs.txt" | sort -u >"$WORK/all-refs.txt"
echo "stdenv_refs=$(wc -l <"$WORK/stdenv-refs.txt") hook_refs=$(wc -l <"$WORK/hook-refs.txt") union_refs=$(wc -l <"$WORK/all-refs.txt")"

# 5. Availability in the allowed offline sources: the seeded image clone in the
#    disposable volume and the retained Run10 store, both read-only.
"$PODMAN" run --rm --network=none --read-only \
  -v "$VOLUME:/nix:ro" -v "$RUN10:/run10:ro" -v "$WORK/all-refs.txt:/refs.txt:ro" \
  --entrypoint=/bin/sh "$IMAGE" -c '
  while IFS= read -r p; do
    if [ -e "$p" ]; then echo "VOLUME_BYTES $p"
    elif [ -e "/run10${p#/nix}" ]; then echo "RUN10_BYTES $p"
    else echo "NO_BYTES $p"; fi
  done < /refs.txt' >"$WORK/bytes.txt"

# 6. Validity rows in the seeded database and the Run10 database. The image has
#    no host Python, so the two databases are copied read-only and queried on
#    the host; neither database is modified.
"$PODMAN" run --rm --network=none --read-only --userns=keep-id --user=0:0 -v "$VOLUME:/target:ro" \
  -v "$WORK:/out:rw" --entrypoint=/bin/sh "$IMAGE" \
  -c 'cp /target/var/nix/db/db.sqlite /out/volume-db.sqlite'
"$PODMAN" run --rm --network=none --read-only --userns=keep-id --user=0:0 -v "$RUN10:/run10:ro" \
  -v "$WORK:/out:rw" --entrypoint=/bin/sh "$IMAGE" \
  -c 'cp /run10/var/nix/db/db.sqlite /out/run10-db.sqlite'
python3 - "$WORK" <<'PY' >"$WORK/rows.txt"
import sqlite3, sys
work = sys.argv[1]
refs = [line.strip() for line in open(f"{work}/all-refs.txt") if line.strip()]
volume = sqlite3.connect(f"file:{work}/volume-db.sqlite?mode=ro", uri=True)
run10 = sqlite3.connect(f"file:{work}/run10-db.sqlite?mode=ro", uri=True)
for path in refs:
    v = volume.execute("select 1 from ValidPaths where path=?", (path,)).fetchone()
    r = run10.execute("select 1 from ValidPaths where path=?", (path,)).fetchone()
    print(("VOLUME_ROW " if v else "") + ("RUN10_ROW " if r else "") + ("NO_ROW " if not (v or r) else "") + path)
PY

grep -h -e '^VOLUME_BYTES ' -e '^RUN10_BYTES ' "$WORK/bytes.txt" \
  | cut -d' ' -f2 | sort -u >"$WORK/available-bytes.txt"
comm -23 "$WORK/all-refs.txt" "$WORK/available-bytes.txt" >"$WORK/missing-refs.txt" || true
echo "missing_bytes_count=$(grep -c . "$WORK/missing-refs.txt" || true)"

# 7. Capture Nix's own refusal for each missing reference. This is a real Nix
#    error from the disposable store, not an inferred one.
: >"$WORK/nix-errors.txt"
while IFS= read -r p; do
  [ -n "$p" ] || continue
  err=$("$PODMAN" run --rm --network=none --user=0:0 -e HOME=/tmp \
    -v "$VOLUME:/nix:rw" --entrypoint=/bin/sh "$IMAGE" \
    -c "nix path-info '$p' || true" 2>&1 | tail -1)
  printf '%s :: %s\n' "$p" "$err" >>"$WORK/nix-errors.txt"
done <"$WORK/missing-refs.txt"

# 7b. Exact five-path check in the image's full /nix/store. This is the only
#     full-image byte check: exactly the five references that have no validity
#     row and no bytes in the seeded closure or the retained store. No crawl.
: >"$WORK/five-path-image-check.txt"
five_absent=0
index=0
for path in "${FIVE_PATHS[@]}"; do
  index=$((index + 1))
  result=$("$PODMAN" run --rm --network=none --read-only --entrypoint=/bin/sh "$IMAGE" -c "
    if [ -e '$path' ]; then
      kind=file
      [ -d '$path' ] && kind=dir
      [ -L '$path' ] && kind=symlink
      printf 'PRESENT type=%s' \"\$kind\"
    else
      printf 'ABSENT'
    fi" 2>&1)
  printf 'utc=%s path=%s result=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$path" "$result" \
    >>"$WORK/five-path-image-check.txt"
  if [ "$result" = "ABSENT" ]; then
    five_absent=$((five_absent + 1))
    continue
  fi
  # Bytes exist: capture the real NAR hash and size on the host.
  "$PODMAN" run --rm --network=none --read-only --entrypoint=/bin/sh "$IMAGE" \
    -c "nix-store --dump '$path'" 2>/dev/null >"$WORK/five-$index.nar"
  printf 'utc=%s path=%s nar_size=%s nar_sha256=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$path" "$(stat -c%s "$WORK/five-$index.nar")" \
    "$(sha256sum "$WORK/five-$index.nar" | cut -d' ' -f1)" \
    >>"$WORK/five-path-image-check.txt"
done

# 8. Verdict: a truthful registration requires real bytes plus a validity row
#    for every reference. If any is absent, stop without registering anything.
if [ -s "$WORK/missing-refs.txt" ] || [ "$five_absent" -gt 0 ]; then
  status=BLOCKED_MISSING_REFERENCES
else
  status=REFERENCES_AVAILABLE
fi
python3 - "$WORK" "$status" "$STDENV_SRI" "$STDENV_HEX" "$HOOK_SRI" "$HOOK_HEX" <<'PY'
import json, os, sys
work, status, stdenv_sri, stdenv_hex, hook_sri, hook_hex = sys.argv[1:7]
def lines(name):
    path = os.path.join(work, name)
    return [line.rstrip("\n") for line in open(path)] if os.path.exists(path) else []
print(json.dumps({
    "status": status,
    "claim": "NAR/reference feasibility in a disposable Nix store; nothing registered or injected",
    "stdenvNarSRI": stdenv_sri,
    "stdenvNarSha256Hex": stdenv_hex,
    "hookNarSRI": hook_sri,
    "hookNarSha256Hex": hook_hex,
    "missingReferences": lines("missing-refs.txt"),
    "nixInvalidPathErrors": lines("nix-errors.txt"),
    "fivePathImageCheck": lines("five-path-image-check.txt"),
    "fivePathImageAbsent": sum(1 for line in lines("five-path-image-check.txt") if line.endswith("result=ABSENT")),
    "blockedSources": ["pinned image full store and database", "retained Run10 store and database", "frozen composition trees"],
    "availability": lines("bytes.txt") + lines("rows.txt"),
}, indent=2, sort_keys=True))
PY
echo "status=$status"
