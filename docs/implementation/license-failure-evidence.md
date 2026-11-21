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
receipt. The builder writes a bounded, exact manifest before cleanup:

```json
{
  "observedRoot": "/nix/store/…",
  "facts": [{
    "storePath": "/nix/store/…",
    "deriver": "/nix/store/….drv",
    "narHash": "sha256-…",
    "outputs": ["/nix/store/…"]
  }],
  "factCount": 1,
  "closureManifestSha256": "…",
  "truncated": false
}
```

The runtime broker returns the manifest in the authenticated failure body. Core
issues the exact identity challenge before dispatch, signs and verifies the
manifest with the broker bearer, and consumes it as a `SpecCacheObservation`
before returning the blocker. The plan remains blocked; no `BuildReceipt`, spec
or licence ACCEPT is fabricated.

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
