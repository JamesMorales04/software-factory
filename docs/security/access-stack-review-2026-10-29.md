# Access stack independent review — 2026-10-29

**Reviewed stack:** `main` → `feature/personal-access-auth` (#36) →
`feature/personal-access-tailscale` (#37) →
`feature/personal-access-pairing-office` (#38, `c8bf930`).
**Reviewers:** Codex `gpt-6.1-sol` (architecture/authorization) and
`gpt-6-luna` (security). Read-only; neither merged.
**Verdict:** **BLOCK all three at the reviewed heads.**

## Sol — blocking findings

1. **#36 HIGH — legacy operator credential survives upgrade.**
   `office/src/coreApi.ts` previously stored the permanent operator token in
   `sessionStorage["factory-api-token"]`. The PR stopped using it but did not
   remove an existing value; device logout/logout-all/recovery do not revoke
   that permanent token. Required: remove the legacy key during frontend
   initialization without reading/logging it; add a seeded-upgrade test.
2. **#37 HIGH — Tailscale boundary exposes the full Core API.**
   The overlay attaches Tailscale to Core's container network while Core still
   listens on `0.0.0.0:8765`; Serve restricts proxied destination, not network
   reachability. Required: isolate the Office-facing listener/network from the
   control listener and add real Podman negative tests from the Tailscale
   network.
3. **#38 HIGH — approved pairing survives revocation.**
   A claimed/approved challenge can be polled after logout-all/revoke-all/
   recovery and issue a new operator-equivalent session; consume and session
   creation are separate transactions. Required: invalidate outstanding
   enrollment authority on global revocation/recovery and fence issuance
   atomically; test delayed polling and concurrent exchange/revocation.
4. **#36 MEDIUM — bootstrap audit coverage incomplete.**

## Luna — blocking findings

- Main branch protection was unverified; the repository currently reports no
  protection (HTTP 404). Prepared policy is recorded in
  `docs/operations/main-branch-protection-2026-10-29.md`.
- Pairing code has ~20 bits of entropy; use `ring::rand::SystemRandom` with
  rejection sampling and add challenge-keyed throttling/concurrency tests.
- Session cookies, CSRF, token digests, pairing poll tokens, Tailscale userspace
  networking and the Office route allowlist were reviewed positively; runtime
  Tailscale/Podman behavior remains NOT_TESTED.

## Promotion order if the Owner authorizes

1. Repair #36, #37, #38 at new heads; rerun tests; re-review the actual diffs.
2. Apply the prepared `main` protection (or owner-approved variant).
3. Merge #36 → #37 → #38 in order, rebasing each on the prior merged main.
4. Promote Wave 3 from a clean rebased branch with full gates and independent
   review.

No merge, branch-protection change or access-stack modification occurred.
