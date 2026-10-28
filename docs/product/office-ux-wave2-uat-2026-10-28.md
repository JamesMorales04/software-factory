# Office UX Wave 2 — owner UAT guide (2026-10-28)

**Code candidate:** `7063321` (runtime/UI); final branch commit may add
documentation and one unreachable grant-lifetime cleanup.
**Branch:** `campaign/office-ux-wave2-integration-20261028`
**Not merged to `main`.** `origin/main` remains `d587252`.

This is a deterministic, developer-preview UAT guide. It does **not** authorize
live provider spending, autonomous issue start or delivery.

## 1. Start the supported product path

```bash
podman compose up -d --build
```

Do not expose a host Podman socket, use `--privileged`, or add host networking.
The host needs no Rust, Node, OpenCode, Codex or Playwright installation.

Open the Office URL printed by Compose (default
`http://127.0.0.1:8894`). Complete first-run owner enrollment with the
one-time code from the init/config volume.

## 2. Project and request

1. **Projects** → import an authorized GitHub repository, or **Local
   repository** using a path inside `FACTORY_PROJECTS_ROOT`.
2. Open the Project summary; review source, frozen base commit, connection
   health and advisory capability discovery. Apply only capabilities you want.
3. **Home** → select the Project and an executable Execution Profile, enter the
   request, and submit.
4. Review the assessment. If a provider route is not executable, stop; do not
   expect fixture fallback in a real deployment.
5. Freeze the plan, then approve the **exact plan hash** through the operator
   boundary. This creates the Campaign and Lead Attempt.

## 3. Execution and independent evidence

1. A running Worker claims the Lead Attempt and produces an exact Git candidate.
2. Open **Campaigns** → Campaign → **Candidates**; inspect the bounded
   per-file diff and exact commit/tree identity.
3. Wait for the independent verifier and semantic-reviewer receipts.
4. Inspect **Quality Gate**; only `ACCEPTED` is a delivery authorization.
5. Open **Evidence** and confirm receipts are bound to the exact candidate.
6. Restart Core/Compose and reload: request, Campaign, candidate, receipts and
   Gate must persist.

## 4. Operator surfaces

- **Decision Inbox:** actions re-read exact revisions; stale items refuse. Only
  Core-backed actions are shown.
- **Issues:** configure issue-intake policy (default OFF), select exact
  synchronized snapshots, acknowledge a finite batch. Reconciliation creates a
  `REVIEW_PLAN` proposal only; it must not start a Campaign or call a provider.
- **Settings:** provider/model discovery, per-role profile routes, runtime
  policy.
- **Language:** switch EN/ES in the header; locale persists. Machine IDs,
  statuses, hashes and Core prose intentionally remain untranslated.
- **PR/checks:** open a PR only for the exact candidate head; merge requires
  explicit confirmation, fresh GitHub checks and an `ACCEPTED` Core Gate.

## 5. Expected limits

- Live provider inference is **BLOCKED / NOT_TESTED** without an owner-supplied
  credential and fresh bounded authorization.
- Compose Office/Chromium acceptance and real GitHub write/merge are
  **NOT_TESTED**.
- Aggregate cross-role model spending is **NOT IMPLEMENTED**; this is the
  remaining independent-review BLOCK. Do not treat profile Campaign limits as a
  whole-request envelope.
- Autonomous issue start remains blocked by ADR-0029, owner approval,
  confinement, accounting and independent review.
- `main` is not protected on GitHub (protection API returns 404). Do not merge
  without explicit owner authorization and the required review.
- Deterministic fixture runs prove lifecycle wiring, not model quality or live
  provider compatibility.
