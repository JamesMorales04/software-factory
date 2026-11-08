# Dependency evaluation — computer-folder import (2026-11-08)

**Status:** ADOPTED for the developer-preview computer-folder import slice.
This record follows [dependency selection](dependency-selection.md).

## Capability

Factory Office must let the owner select a folder from the operating system and
register it as a Project without a host bind mount. The browser can read the
selected directory through the File System Access API, but Core cannot receive a
live directory handle. The bounded transfer is a ZIP archive posted to Core,
which extracts it into managed storage with traversal, symlink, entry-count and
size protections.

## Browser archive writer

**Selected:** `fflate` 0.8.2 (MIT).

- Small, synchronous ZIP writer with no native dependency; runs in Chromium.
- Already used only for export of the selected directory; it does not decide
  Project authorization or path policy.
- Alternatives: JSZip (larger, similar capability), custom ZIP writer (rejected:
  unnecessary binary-format maintenance).
- Exact version pinned in `office/package.json`/`package-lock.json`.

## Core archive reader

**Selected:** `zip` 9.4.2 (MIT), `deflate` feature only.

- Maintained Rust ZIP reader; avoids a custom archive parser.
- Core still owns the security boundary: `enclosed_name` rejects traversal,
  symlinks are rejected from the Unix mode, entry count is capped at 20,000,
  per-file size at 64 MiB and expanded total at 512 MiB.
- Alternatives: `async_zip` (tokio-oriented, larger API surface), shelling out
  to `unzip` (host tool dependency, rejected), custom parser (rejected).
- Exact version pinned in `native/Cargo.toml`/`Cargo.lock`; upgraded from 2.6.1 to 9.4.2 to clear four RustSec advisories.
- Rust 1.90 compatibility and Linux build verified in the canonical Compose
  image; no native/system library is required.
- Residual risk: ZIP is a transfer format, not an authorization grant. The
  extracted directory is validated through the existing managed-root and Git
  boundary before Project registration.

## Verification

- `project_import::tests::imports_an_uploaded_folder_archive_and_initializes_git`
  covers extraction, Git initialization and managed-root containment.
- The browser component test covers picker traversal, skipped dependency
  directories and the Core upload call.
- Live Compose browser validation is recorded in
  [office-autonomy-ux-2026-11-08](../evidence/office-autonomy-ux-2026-11-08/manifest.json).
