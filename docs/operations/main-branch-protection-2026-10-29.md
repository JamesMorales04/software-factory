# `main` branch protection — prepared policy (2026-10-29)

**Status:** prepared, **not applied**. Applying it changes repository settings and
requires explicit Product Owner authorization.

## Observed state

`GET /repos/JamesMorales04/software-factory/branches/main/protection` returns
HTTP 404: `main` is currently unprotected. PRs #36, #37 and #38 are open drafts
and currently report `MERGEABLE`.

## Proposed protection

| Setting | Value |
|---|---|
| Require a pull request before merging | yes |
| Required approving reviews | 0 (single-maintainer repository; see note) |
| Dismiss stale approvals on new commits | yes |
| Require approval of the most recent push | no (owner cannot self-approve) |
| Require conversation resolution | yes |
| Require linear history | yes |
| Require status checks to pass | yes |
| Require branches to be up to date | yes |
| Required checks | `Governance consistency / architecture-consistency`, `Native Rust / rust`, `Native Rust / compose-policy`, `Native Rust / container-dev` |
| Allow force pushes | no |
| Allow deletions | no |
| Enforce for administrators | yes |
| Restrict who can push | no extra restriction (PR-only flow) |

**Single-maintainer constraint:** GitHub does not allow an author to approve their
own pull request, and this repository currently has only `JamesMorales04` as a
collaborator. A required approval count of 1 with `enforce_admins: true` would
make every legitimate merge impossible. The safe feasible policy is therefore
**0 required GitHub approvals + strict status checks + conversation resolution +
linear history**, with independent Sol/Luna review recorded in the PR evidence.
If a second trusted collaborator/reviewer is added, raise the count to 1 and
keep `enforce_admins: true`.

The manual `Compose end-to-end (manual)`, `Native Rust Podman certification` and
`Native Rust provider compatibility` workflows are release gates, not
PR-triggered checks; they cannot be required on every PR without a workflow
trigger change. Record their successful runs as release evidence.

## Exact command (do not run without owner approval)

```bash
gh api -X PUT repos/JamesMorales04/software-factory/branches/main/protection \
  -H "Accept: application/vnd.github+json" \
  --input docs/operations/main-branch-protection-payload.json
```

The payload is stored beside this document. It contains no secrets.

## Required promotion order

1. **PR #36** (`feature/personal-access-auth`) → `main`: owner bootstrap,
   managed browser sessions, CSRF/cookies. Rebase on current `main`, rerun
   Native Rust + Governance, obtain independent Sol/Luna approval.
2. **PR #37** (`feature/personal-access-tailscale`) → `main`: optional
   Office-only private access. Rebase on the merged #36, rerun checks.
3. **PR #38** (`feature/personal-access-pairing-office`) → `main`: QR device
   pairing and Access & Devices. Rebase on merged #37, rerun checks.
4. Apply this protection immediately after the first authorized merge, or
   earlier if the Owner prefers; the settings are independent of the stack.
5. Only after #36–#38 are on `main`, open a separate Wave 3 promotion PR from
   `campaign/office-ux-wave3-20261028` (or a clean rebased branch), rerun the
   full Rust/Office/Compose gates, and obtain Sol/Luna plus Owner approval.

No step in this document authorizes a merge or a settings change.
