# Refused licence-gate identity evidence

Status: live-UAT repair record. No authority is granted by this evidence.

## Observed blocker

The new plan revision `45f74096…` was `CORE_VALIDATED` with the current
catalogue `edef68e1…`. The retained environment review `c2c8b429…` was
`SETTLED` and `APPROVED`. The builder then refused the exact closure path
`/nix/store/36qzll…-ed-1.21.1` with `ENVIRONMENT_LICENSE_UNKNOWN`.

The broker logged:

```text
closure licence inventory is unavailable before environment cleanup
```

The first attempted build had already removed the realised cache. No new
identity-facts receipt or spec was persisted; the old receipt `475c1f95…` was
bound to the old plan `c6cb4499…`. The licence evidence review could therefore
not run (`LICENSE_IDENTITY_FACTS_REQUIRED`).

## Repair principle

A refused licence gate is **evidence**, not a grant and not a successful build
receipt. The builder writes only a bounded non-authority
`realisation.json` pointer before cleanup:

```json
{
  "schemaVersion": 1,
  "planSha256": "…",
  "specSha256": "…",
  "storePath": "/nix/store/…",
  "closureManifestSha256": "…",
  "status": "REALISED"
}
```

Core issues the exact identity challenge before dispatch. The runtime broker
reads the pointer, validates it against the exact plan/spec, re-runs its fixed
`nix path-info`/`nix derivation show` queries over the persistent store, builds
the manifest itself and signs the envelope. Core only verifies and consumes the
broker receipt. A builder-authored fact manifest is never signed or trusted.
The plan remains blocked; no `BuildReceipt`, spec or licence ACCEPT is
fabricated.

## Resolve-stage extension

A resolve-stage refusal has no spec yet. The supported contract uses an
`IdentityFactsStage::Resolution` challenge with a canonical
`resolutionKey` over the exact `RealisationObservation`, a persistent
plan-keyed resolve cache, and a broker fallback that re-observes the exact root
with the unchanged fixed Nix queries before signing. A `RESOLUTION` receipt is
accepted only with `SpecCacheObservation`.

The retry resolve/build uses only the persisted exact ACCEPT decision through
`verify_license_decision_overlay`; there is no second approval engine, no
static asset grant and no old-plan/old-receipt reuse.

## Current state

The live stack remains on the verified `05e71714` images; the plan revision,
review and three settled calls are preserved. No a53f image is built and no
further model call is made until the corrected backend and a fresh build GO.
