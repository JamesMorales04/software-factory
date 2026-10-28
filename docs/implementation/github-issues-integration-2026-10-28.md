# GitHub Issues integration — campaign lane #50

**Status:** implemented in the Office UX campaign lane
(`campaign/office-ux-issues-20261028`), based on
`campaign/office-ux-baseline-20261028` at `9f341db`.

This document describes the read-only GitHub issue snapshot foundation. It is
not evidence of a live GitHub call, a governed issue board, or delivery
authorization. An authorized real GitHub API verification remains `NOT_TESTED`
unless it is recorded with a candidate-bound artifact.

## Scope

Delivered:

- an encrypted, revocable GitHub connection owned by Core;
- one GitHub repository binding per Project;
- a bounded GitHub REST client for issue synchronization;
- durable issue snapshots with stable `(project, repository, number)` identity;
- truthful sync state (`OK`, `STALE`, `RATE_LIMITED`, `DENIED`, `NOT_FOUND`,
  `ERROR`, `REVOKED`, `NEVER_SYNCED`) and stale/truncated indicators;
- an Office `Issues` view for connection, binding, sync, search, filtering,
  list and safe plain-text detail.

Explicitly deferred:

- governed intake/board ordering, priorities and drag-and-drop;
- Action/Campaign relationships and issue-to-work conversion;
- writing to GitHub (comments, labels, closure, creation);
- issue comments synchronization;
- agent consumption of issue text;
- real authorized GitHub acceptance.

## Trust boundary

Core owns the connection, repository binding, synchronization, snapshots and
query policy. Office is an authenticated client; it never receives the token,
never writes SQLite and never grants authority. Issue title/body/label text is
untrusted data. It is stored as bounded text, rendered as plain React text, and
never passed to a model, agent, shell or instruction interpreter.

## Schema — migration `0028_github_integrations.sql`

Migration `0026` and `0027` are reserved by the execution-profile and intake
lanes, so this lane allocates `0028`. `sqlx` versioned migrations preserve
existing data; the new tables are additive.

| Table | Purpose |
|---|---|
| `github_connections` | singleton connection: login, scopes, AES-256-GCM ciphertext/nonce/fingerprint, enabled, revision, verification/revocation times |
| `github_connection_audit` | bounded (last 200 rows) configure/revoke audit without credential material |
| `project_github_bindings` | one `owner/repo` + default branch per Project |
| `github_issues` | bounded snapshots keyed by `(project_id, repo_full_name, number)` with sync mark and content hash |
| `github_sync_state` | per-Project status, ETag, last success/attempt, rate-limit reset, counts, stale/truncated |

No table contains a plaintext token. `github_issues.body` is capped at 64 KiB
and records `body_truncated`.

## Core endpoints

All routes require the operator bearer token and are mounted in both the
control API router and the Office API router (and therefore under `/core-api`).
Office requests also use the device-session cookie and CSRF header.

| Method | Path | Semantics |
|---|---|---|
| `PUT` | `/integrations/github` | body `{"token":"..."}`. Verifies the token with `GET /user`, records login and reported scopes, encrypts and stores it. Invalid credentials are rejected and not stored. |
| `GET` | `/integrations/github` | returns `{connection, audit}`; never ciphertext, nonce or token |
| `DELETE` | `/integrations/github` | idempotently clears the credential, disables the connection, marks every sync state `REVOKED` and stops synchronization |
| `GET` | `/integrations/github/repositories?query=` | bounded live `/user/repos` listing for binding; private repositories appear only when the token can access them |
| `POST` | `/projects/{id}/github` | body `{owner, repo, default_branch}`; validates coordinates, rebinds, resets sync state and removes issue rows for a previous repository |
| `POST` | `/projects/{id}/issues/sync` | one bounded synchronization; returns the durable sync state. Upstream failure is recorded truthfully rather than fabricated as success |
| `GET` | `/projects/{id}/issues?state=&label=&query=&limit=&offset=` | paged snapshot with `synced_at`, `stale`, `sync_status`, `last_error`, `truncated` and binding |
| `GET` | `/projects/{id}/issues/{number}` | one snapshot including body and `body_truncated` |

Status mapping:

- `401`/`403`/`404` from GitHub → `DENIED`/`DENIED`/`NOT_FOUND` sync state;
- `403` with `x-ratelimit-remaining: 0`, `retry-after`, or `429` →
  `RATE_LIMITED` with a truthful HTTP status and next-allowed time;
- redirects → refused without forwarding `Authorization`;
- malformed/oversized responses → `ERROR`;
- a revoked or unconfigured connection → `409` before any GitHub call.

## Bounded client

`native/src/github.rs`:

- explicit base `https://api.github.com`;
- test-only loopback base requires `FACTORY_ALLOW_TEST_UPSTREAM=1` and
  `FACTORY_GITHUB_API_BASE` with a loopback host; production never accepts a
  base URL from Office or a request body;
- 20 s request timeout, no proxy, redirects disabled;
- maximum 10 issue pages × 100 items, maximum 16 MiB per response, 64 KiB body,
  512-byte title, 50 labels, 100 repositories over 2 pages;
- ETag/`If-None-Match` on the first page; `304` is a successful no-change sync;
- pull requests returned by the issues endpoint are excluded;
- field-level normalization, HTTPS-only detail links and GitHub label colors;
- errors never include response bodies or credentials.

## Synchronization

`native/src/github_sync.rs` performs one synchronization:

1. refuse when the connection is disabled/revoked;
2. load the Project binding and previous ETag;
3. fetch up to the bounded page limit;
4. on `not_modified`, refresh `last_synced_at` and keep the snapshot;
5. otherwise upsert every issue with a new sync mark, then delete rows not
   present **only when all pages were fetched**; a truncated sync suspends
   deletion reconciliation;
6. record counts, ETag, success/attempt times and stale flag.

A rebind during an in-flight sync conflicts rather than writing issues under the
old repository. A revocation during an in-flight sync is rejected inside the
write transaction, so it cannot resurrect a revoked connection.

## Office

`office/src/views/Issues.tsx` exports `Issues` and imports a view-local
`issues.css`. It provides:

- Project selector;
- connection state, login, reported scopes, fingerprint and bounded audit;
- explicit connect/replace token and two-step disconnect;
- repository binding with optional live repository browsing (private repos
  labelled);
- sync action with truthful status, last-sync time, stale/rate-limit/truncated
  states and last-sync counts;
- state/label/text filters and bounded pagination;
- issue list and detail with labels, author, comment count and plain-text body;
- empty states for unbound Project, never-synced repository, no matches and no
  reported issues; loading, error and rate-limit states.

Token input is `type="password"`, cleared after verification and never written
to browser storage.

**Route wiring (integration owner):**

```tsx
// office/src/App.tsx
import { Issues } from "./views/Issues";
<Route path="/issues" element={<Issues />} />
```

```ts
// office/src/navigation.ts
{
  to: "/issues",
  label: "Issues",
  icon: ListChecks,
  keywords: "github issues sync repository labels triage",
}
```

`App.tsx` and `navigation.ts` were intentionally not edited by this lane.

## Verification performed

Deterministic, in this worktree:

```text
cargo fmt --all -- --check
cargo test -p factory-native --lib                 # 282 passed, 2 ignored
cargo test -p factory-native --lib github          # 14 passed
cargo test -p factory-native --bin factory-core    # 14 passed
cargo clippy -p factory-native --all-targets -- -D warnings
npm --prefix office run typecheck
npm --prefix office run lint
npm --prefix office run test                       # 16 files, 72 tests passed
npm --prefix office run test -- --run src/views/Issues.test.tsx   # 6 passed
npm --prefix office run build
```

The fake-GitHub tests cover authenticated private-repository listing,
401/403/404, 403/429 rate limits, pagination, pull-request exclusion, ETag/304,
redirect refusal, token redaction, duplicate sync, closure/deletion
reconciliation, stale snapshots, revocation (including in-flight rejection),
cross-Project identity isolation, label/search filters and wildcard escaping.

A local API smoke (fake GitHub on `127.0.0.1:19090`, Core on
`127.0.0.1:18765`, no real GitHub credential) exercised:

- `PUT /integrations/github` → configured, `login=octo`, scopes recorded;
- invalid token shape → `400`, nothing stored;
- `GET /integrations/github/repositories?query=widg` → private repository;
- Project registration and `POST /projects/{id}/github` → binding;
- first sync → `OK`, fetched 2, created 2; duplicate sync → created 0,
  updated 2, total 2;
- `GET /projects/{id}/issues?state=open`, `state=closed&label=bug` and
  `query=Flaky` returned only matching rows;
- detail returned the body including a literal `<script>` string (stored as
  text, not executed);
- `DELETE /integrations/github` → configured false, sync state `REVOKED`,
  stale true, subsequent sync `409`;
- credential ciphertext length 84, nonce 24; plaintext token absent from the
  database and from every API response;
- unauthenticated `GET /integrations/github` → `401`.

**NOT_TESTED:** a real authorized GitHub API call. No developer `gh` token was
read or used in product code. Real browser acceptance and Compose E2E for the
`/issues` route are also pending integration wiring and authorization.

## Residual risks

- GitHub API behavior for Fine-grained tokens and enterprise restrictions is
  covered only by fake-server tests until a real authorized call is recorded.
- ETag handling conditions on page 1 only; issue deletion on later pages is
  reconciled by the complete-scan marker, and a truncated scan intentionally
  suspends deletion.
- The Office view is read-only for GitHub issue content; governed intake,
  board ordering and Action/Campaign relationships remain future work.
- No independent security review or Codex review has run on this lane.
