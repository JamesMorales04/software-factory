## Change summary

<!-- Describe the observable change and relevant verification. -->

## Architecture impact

For protected changes, replace every placeholder below with the category, a concise impact rationale, and the exact current base and head SHAs shown by the PR check. The check report is bound to that pair; update the SHAs after either side changes. Ordinary application changes may leave this section untouched because the diff classifier records why no assessment was required.

<!-- architecture-impact:start -->
category: <architecture|authorization|security|quality|evidence|delivery|ci|runtime|skills|governance>
rationale: <at least 40 characters describing affected guarantees and intended behavior>
base_sha: <40-character base commit SHA>
head_sha: <40-character PR head SHA>
<!-- architecture-impact:end -->

## Container-native impact

Required for changes to `compose*.yaml`, `deploy/`, `.devcontainer/`,
`native/src/bin/`, `scripts/compose-*`, container-facing workflows and runtime
dependency manifests. Use `impact: none` only for a genuinely internal change
with a documented rationale. Runtime changes require a real Podman result, not
mock-only evidence.

<!-- container-native-impact:start -->
impact: <changed|none>
# If impact is none, remove the five fields below and keep only impact, rationale, base_sha and head_sha.
services: <comma-separated services or "none">
commands: <container-native commands actually run>
real_podman: <real Podman/Compose test or "none" with rationale>
negative_tests: <policy/security negatives or "none" with rationale>
not_tested: <explicit remaining scope>
rationale: <at least 40 characters describing the container and trust impact>
base_sha: <40-character base commit SHA>
head_sha: <40-character PR head SHA>
<!-- container-native-impact:end -->

## Office impact

Required for `office/`, Office routes/components, Office product docs, parity
matrix changes or user-facing Core state/action changes. Use `impact: none` only
for an internal refactor with a documented rationale. Do not add a decorative
button to satisfy this section.

<!-- office-impact:start -->
impact: <changed|none>
# If impact is none, remove the five fields below and keep only impact, rationale, base_sha and head_sha.
capabilities: <CAP-... IDs or "none">
routes: <comma-separated Office routes or "none">
core_api: <comma-separated METHOD /path entries or "none">
user_actions: <real operator actions or "none">
states: <loading/empty/error/offline/unauthorized etc.>
acceptance: <real browser test paths or "none" with rationale>
rationale: <at least 40 characters describing the user journey and evidence>
base_sha: <40-character base commit SHA>
head_sha: <40-character PR head SHA>
<!-- office-impact:end -->
