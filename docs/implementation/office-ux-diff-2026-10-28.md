# Office UX Wave 2 — Campaign diff and GitHub PR lane

**Parent:** #39. **Branch:** `campaign/office-ux-diff-20261028`.
**Baseline:** `924e026`. **Code candidate:** `b5be0f1` (Core) and
`541edfc` (Office). Evidence:
[office-campaign-diff-pr-2026-10-28.json](../evidence/office-campaign-diff-pr-2026-10-28.json).
This is implementer evidence, not independent acceptance.

## Delivered

### Bounded candidate file diff

`native/src/candidate_diff.rs` projects the Git diff from the exact frozen base
commit to the exact candidate commit/tree:

- repository identity is revalidated against the Project binding before any Git
  object is read;
- base and candidate must be full Git object IDs; the candidate tree must equal
  the recorded tree; the Candidate ActionSpec hash must equal the frozen Action;
- changed files are capped at 200, text blobs at 512 KiB, each patch at 128 KiB,
  total patches at 512 KiB and paths at 1,000 bytes;
- binary, large, symlink/gitlink, non-UTF-8 and budget-omitted patches are
  explicit states; no content is inferred;
- status, additions/deletions, modes and unified patch bodies come from Git;
  symlinks and gitlinks are never followed and no candidate code executes;
- the response retains the prior path/stat summary and adds the exact candidate
  and Gate identity.

`GET /campaigns/{id}/candidates/{candidate_id}/diff` remains the authenticated
operator route. A stale identity, missing revision, unsafe path or non-descended
candidate returns a typed conflict/error, never a fabricated diff.

### GitHub pull request operations

The existing encrypted GitHub connection is reused. Tokens stay in Core; Office
never receives or stores one. Routes:

| Route | Effect |
|---|---|
| `GET /projects/{id}/github` | read the Project owner/repository/default branch binding |
| `GET /campaigns/{id}/candidates/{candidate_id}/pull-request` | fresh PR, mergeability, check-run/commit-status read |
| `POST …/pull-request` | open a PR only when the remote head branch resolves to the exact candidate commit |
| `GET …/pull-request/diff` | bounded GitHub PR diff (512 KiB) |
| `GET/POST …/pull-request/comments` | bounded issue/review comments; post a plain-text comment |
| `POST …/pull-request/review` | request named GitHub reviewers; no approval is inferred |
| `POST …/pull-request/merge` | explicit-confirmation merge with the expected head pinned |

Merge is blocked unless Core has just read an open PR whose head equals the
candidate commit, GitHub reports `mergeable=true` and `mergeable_state=clean`,
and the fresh check/status read is complete and successful. GitHub remains
authoritative for branch protection and required checks. A GitHub 405/409/422,
insufficient credential permission, changed head, pending/failed check or an
unbound Project is a typed blocked result. There is no auto-merge and no retry.

### Office

Campaign → Candidates now contains:

- a changed-file list with status/additions/deletions and patch-omission labels;
- per-file unified patches with add/delete/hunk styling, line-wrap control and
  keyboard-scrollable regions;
- base/candidate/tree revision facts, copyable SHAs, candidate and Gate badges;
- loading, empty, error, file-list truncation, patch truncation, binary, large,
  non-file and budget states;
- a PR panel for the Project binding, fresh PR/check read, bounded PR diff,
  comments, reviewer request, exact-head open form and explicitly confirmed
  merge. Unbound, denied, blocked and pending states are surfaced, never faked.

## Verification

- Rust: `cargo fmt --all --check`; workspace clippy with `-D warnings`; 7
  candidate-diff tests; 14 GitHub client tests; 22 `factory-core` tests.
- Office: typecheck, Biome lint, 148 component tests (26 files) and production
  build pass.
- Rust lib suite: 348 passed after filtering one pre-existing
  environment-dependent browser log-path test. The full suite still has two
  pre-existing OpenCode sandbox failures.
- No migration was added.

## Not tested / blockers

- Real GitHub API and credential-permission behavior: **NOT_TESTED**.
- Real browser acceptance (1440/1024/834/390/320), axe/screen reader and
  dark/compact visual review: **NOT_TESTED**.
- Branch creation/push is not implemented; a missing or mismatched remote head
  is a typed blocked state.
- Independent verification, semantic review, Quality Gate acceptance and
  Product Owner merge authorization remain outside this lane.

## Next task

Run the lane against an authorized disposable GitHub repository with a real
credential and a pushed candidate branch: read a PR, observe checks and branch
protection, post a comment, request a review, then exercise a blocked merge and
a permitted merge only if the owner authorizes it. Record the exact candidate,
repository, PR, check results and permissions; do not infer live success from
the fake-upstream tests.
