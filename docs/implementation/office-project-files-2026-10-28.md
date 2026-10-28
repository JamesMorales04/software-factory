# Office project files projection (2026-10-28)

**Status:** Implemented in the Office UX campaign lane for issue #44; pending
integration-owner route wiring and independent acceptance. This document
describes the bounded read-only projection. It is not evidence of certified
product acceptance.

## Purpose

Give the Product Owner a truthful, read-only repository browser for an exact
Project revision without terminal access. Git remains the only documentation
source: Core reads objects from the registered Project repository and Office
never writes SQLite, checks out a worktree or creates a parallel documentation
database.

## Trust boundary

- Core resolves the Project row and revalidates its repository identity against
  the frozen registration before reading objects.
- Every request is addressed by a full 40- or 64-character Git commit ID. Branch
  names are returned only as informational revision choices; they are not
  accepted as a browse `ref`.
- Files are read from Git trees and blobs, not from the working directory.
  Symlinks and gitlinks are classified and never followed.
- Responses contain commit, tree and blob identity plus a provenance label. They
  never include a caller-supplied host path.
- All routes require the existing operator/device session authorization, the
  same as the other Project routes.

## Endpoints

| Route | Query | Result |
|---|---|---|
| `GET /projects/{id}/files` | `ref` (optional full commit ID), `path` (optional directory) | Immediate tree entries, revision identity, truncation and skipped-name counts |
| `GET /projects/{id}/file` | `ref` (optional full commit ID), `path` (required file) | Bounded UTF-8 content or an explicit binary/large/symlink omission, plus mode, size, blob and line count |
| `GET /projects/{id}/refs` | none | Frozen base commit, symbolic HEAD branch and bounded local branch refs |

A missing `ref` uses the Project's frozen `base_commit`. An unknown or stale
commit returns `404`; an identity mismatch returns `409`; an unavailable
repository returns `503`; invalid paths or refs return `400`.

## Bounds

| Bound | Value |
|---|---|
| Tree entries per projection | 500 |
| File content | 512 KiB |
| Repository-relative path | 1,000 bytes |
| Local branch refs | 200 |
| Git error detail exposed to the client | none for engine/I/O failures |

Tree entries beyond the bound set `truncated: true`. Names that cannot be
represented safely are counted in `skippedEntries` instead of being silently
rewritten. A file above the content bound returns metadata with
`omitted: "TOO_LARGE"`. NUL bytes or invalid UTF-8 return `omitted: "BINARY"`.
Symlink and submodule entries return `omitted: "NOT_A_FILE"` and are never read.

## Path and ref validation

- Reject empty file paths, absolute paths, trailing slashes, backslashes,
  control characters, `.`/`..` components and `.git` components.
- Pass paths as literal Git pathspecs after `--`; never interpolate a shell.
- Reject non-commit refs, abbreviated object IDs, branch names and option-like
  values before invoking Git.
- Revalidate the registered repository identity on every projection.

## Office experience

Component: `office/src/views/ProjectFiles.tsx`.

Required route wiring (integration owner):

```tsx
import { ProjectFiles } from "./views/ProjectFiles";

<Route
  path="/projects/:projectId/files"
  element={<ProjectFiles />}
/>
```

`navigation.ts` should also map a `/projects/{id}/files` pathname to the label
`Project files`; the existing `/projects` sidebar item already highlights
nested Project paths. `Projects` links to `/projects/{id}/files` with a
**Browse files** action. The
view provides directory-by-directory tree navigation, breadcrumbs, keyboard
arrow/Home/End movement, current-directory filtering, a revision selector fed
by exact commits, provenance and metadata, line-numbered source, a safe Markdown
subset preview (no raw HTML and no `dangerouslySetInnerHTML`), and explicit
empty, binary, large, symlink, error, missing-project and stale-repository
states.

Deep-link parameters: `ref` (full commit), `path` (directory) and `file`
(repository-relative file path). Back/forward reconstructs the same view.

## Deliberate limits

- No repository-wide recursive search; filtering applies to the loaded directory.
- No syntax highlighting, editing, download, full history, blame, diff or IDE
  behavior.
- No symlink or submodule content.
- No branch/tag browsing beyond local branch refs.
- No browser acceptance against the real Compose stack in this lane; component
  tests and Core route tests are the recorded evidence.

## Parity proposal

The integration owner should register this capability in
`docs/product/office-parity.json` (for example `CAP-055`) with routes
`/projects` and `/projects/:projectId/files`, Core API `GET
/projects/{id}/files`, `GET /projects/{id}/file` and `GET /projects/{id}/refs`,
and the states listed above. This document does not amend the frozen campaign
contract or claim that the route is wired.
