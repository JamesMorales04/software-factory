# Progress integration — 2026-11-23

This is an integration checkpoint requested by the Owner, not product certification
or an autonomous-flow acceptance record. Delivery remains subject to the repository
Quality Baseline and exact-head checks.

## Included source cuts

| Workstream | Source commit | Integration scope |
| --- | --- | --- |
| Core, builder and licence contracts | `8103528c024cf34792aff24a7c17e05abbabc236` | Includes reproducible builder bundles, authenticated identity facts, exact review replay, v4 licensing-fact/text provenance, bounded autopilot licence handling, request-scoped funding projection and declaration-derived admission. |
| Office funding | `2ebb880932740e151ef5baa91e547bb57d361a61` | Request funding observation and explicitly unscoped provider status. Core projection commit `580b1f70` is already incorporated in the Core cut. |
| Office licence provenance | `b6cb4bcf3bd8ba733659adef9ab2e8b1b8afe4c0` | Display the selected licensing fact and derived text separately. |
| Browser harness | `c3cd2d424ccd00ff32ebb4751f0fab2119dcda50` | One-prompt and negative-lane driver, contract tests and Compose launcher; source base `f4d883b5`. Runtime execution remains pending. |

Only committed source and its repository documentation are included. Local audit
directories, model responses, databases, credentials and scratch outputs are not
part of this checkpoint. Later speculative licence-authority UI design documents
are not included as an implemented feature.

## Observed technical progress

The earlier bounded UAT settled eight model calls. The environment review approved
plan revision 6. A signed SPEC identity-facts receipt was observed after the builder
reported `ENVIRONMENT_LICENSE_UNKNOWN` for `ed`. The v4 deterministic projection
then recorded an exact ACCEPT decision using the existing settled licence review,
without a ninth model call. It retained the selected `ed.h` licensing fact and
derived the compatible `COPYING` text from authenticated source evidence.

That UAT reached **eight calls, two decisions and zero persisted environment
specifications**. It did not reach freeze, Campaign completion or end-to-end restart
acceptance. Its installed builder was the earlier v3 bundle; the v4 source in this
checkpoint does not retroactively change that runtime or its plan bindings.

## Verification and remaining gates

The workstream authors reported passing Rust library/Core-bin tests, focused
builder/funding tests, Office component/type/build checks and 14 harness contract
tests on their respective source cuts. These reports are provenance, not a passing
result for this integrated head. The PR must record integrated checks separately.

The following remain open and block a claim of completed autonomy:

- A complete licence-gate integration test through decision, one freeze retry and
  restart recovery, with no duplicate model dispatch.
- The real builder trigger needed by the positive one-prompt lane.
- The Core-backed licence-authority projection and explicit Owner UI flow needed
  by the browser lane; a proposed DTO field is not an implemented API.
- Real browser, responsive and accessibility acceptance of the changed Office
  funding and licence-provenance surfaces.
- A fresh one-prompt journey through exact Campaign/Gate/result receipts and a
  real Core restart, verified independently against the frozen integrated head.
- Repository-wide lint and CI, independent review and the deterministic Quality
  Gate for delivery. The Office author reported existing lint diagnostics; those
  are not waived by this checkpoint.

Execution policy, subscription admission and licence delegation remain separate
authorities. R0 or subscription confirmation cannot fabricate licence authority.
The bounded Owner grant is project/pin/use/criterion/revision/deadline scoped; the
Lead decision binds the exact plan/spec/identity receipt. Missing or invalid
authority must produce a truthful wait or stop, never an implicit grant.
