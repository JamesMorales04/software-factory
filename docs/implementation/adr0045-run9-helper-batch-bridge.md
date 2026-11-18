# ADR-0045 bridge — exact approved helper batch (R2, builder-only)

**Status:** FROZEN CANDIDATE for Root review. No integration into the E2E checkout,
no image build, no Run9. Base: `fc1375028b29483a1299ab891a743806a326f10b`
(tree `4d5346d81130ce03eecd39a65bcccdceac96c6ba`), branch
`factory/adr0045-run9-bridge`.

## Scope

Authorize, in the environment builder only, the exact batch approved by the
Owner: **12 new** deriver-less stdenv helpers plus the **2 legacy** paths, for
the single pin:

`github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d`

Identity source: `deriverless-closure-inventory.json` sha256
`6a355fcca7f3fee37b7e9f2cc9c01c4cf71a547d87b75492cc6888cd11397c50`
(73 closure entries, 14 deriver-less, 12 not allowed before this change).

## Invariants

- One table, `APPROVED_HELPER_IDENTITIES`, scoped to the exact pin.
- A new identity is accepted only with exact **path**, **NAR hash**,
  **referrer-set hash** (reviewed referrers `stdenv-linux` and
  `nix-shell-env`), a **regular file** and an **empty deriver**.
- The 2 legacy paths keep their previous path-only acceptance and are derived
  from the same table (`legacy: true`).
- No suffix, pattern, name-only or plan/workspace-provided identity is
  accepted. Unknown deriver-less paths fail closed with
  `ENVIRONMENT_LICENSE_UNKNOWN`.
- No License Gate, unfree, package, catalogue or closure-policy change.
- The builder remains untrusted; the table is code, not plan data.

## Implementation

- `deploy/environment-builder/environment_builder/cli.py`
  - replaces the 2-path allowlist with the exact 14-identity table;
  - adds `_classify_deriverless_helpers` with the deterministic predicate;
  - reuses the candidate facts from diagnostic commit `30357cb0` (referrer-set
    hash and regular-file fact), only where coherent with this slice;
  - keeps the existing bounded failure evidence when a path is unknown.
- `deploy/environment-builder/environment_builder/nix.py`
  - keeps the closure manifest `references` field from `30357cb0`, required for
    the referrer-set hash.
- `deploy/environment-builder/tests/test_builder.py`
  - positive: the entire exact closure (73 entries) accepts 14 helpers, 12 new;
  - negative: unknown path, NAR mismatch, referrer-set mismatch, non-regular
    file, symlink, pin change, suffix/pattern;
  - legacy paths remain accepted.
- `deploy/environment-builder/tests/fixtures/deriverless-closure-inventory.json`
  is an exact copy of the evidence inventory (sha256 above).

## Verification

Run in the canonical builder image, network disabled:

```
podman run --rm --network none --entrypoint python3 \
  -v <worktree>/deploy/environment-builder:/src:ro -w /src \
  localhost/software-factory-environment-builder:dev \
  -m unittest discover -s tests -p 'test_builder.py' -v
```

Result required before any build/GO: **108/108 OK**, `py_compile` OK, offline.

## Explicit non-goals

- No Core/ADR-0045 rule storage, migration, Office surface or capability change.
- No environment-builder image build; no runtime image build.
- No Run9, model call, admission, stack change, Run7/Run8 cleanup or 8893 access.
- No push, merge or deploy.
