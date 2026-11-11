# Nix/devenv license findings

**Status:** WP0 evidence — 2026-11-10. This is a factual license review for the
environment program, not a legal opinion. See the disclaimer at the end.

**Scope:** Nix, nixpkgs and devenv as the environment substrate selected by
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md),
plus the optional hosted-services question (Devenv Cloud/Cachix). It does not
review every nixpkgs package a future project may select; the License Gate must
do that per package.

**Method:** fetched the official repository license files and the upstream
project/service pages on 2026-11-10. URLs and quoted license families are below.
The fetch is a point-in-time observation; a future upstream relicensing requires
a new review.

## 1. Findings

| Component | Official source | License finding | Commercial-use conclusion |
| --- | --- | --- | --- |
| Nix | <https://github.com/NixOS/nix/blob/master/COPYING> | GNU Lesser General Public License, version 2.1 (LGPL-2.1). The `COPYING` file is the LGPL-2.1 text. The reviewed source files did not show a per-file "or later" grant; treat the exact release as LGPL-2.1 and verify the release's headers before relying on any later-version option. | Commercial use is permitted. If Factory distributes a modified Nix binary, it must provide the corresponding source and a relinkable form and preserve notices. Factory invokes Nix as a separate executable/tool and does not link Nix libraries into Factory code, so the LGPL does not automatically cover Factory's separate source. This is a factual reading, not a legal opinion. |
| nixpkgs | <https://github.com/NixOS/nixpkgs/blob/master/COPYING> | MIT. The file is the standard MIT permission text with the copyright notice "2003-2026 Eelco Dolstra and the Nixpkgs/NixOS contributors". | Commercial use is permitted. Preserve the copyright and permission notice in copies/substantial portions. The MIT license of nixpkgs does **not** license the individual packages it contains; each package has its own license. |
| devenv | <https://github.com/cachix/devenv/blob/main/LICENSE> and <https://github.com/cachix/devenv/blob/main/Cargo.toml> | Apache License 2.0 (`Cargo.toml` workspace `license = "APACHE-2.0"`). | Commercial use is permitted. Retain copyright/patent/trademark/attribution notices, include any `NOTICE` text, mark modified files and comply with the patent-termination clause. |
| Devenv Cloud | <https://devenv.sh/cloud/> | A separate hosted service ("private beta" at fetch time), not part of the open-source CLI license. | Not required. Do not make it a product dependency, credential or mandatory account. |
| Cachix | <https://www.cachix.org/pricing> | A separate commercial hosted binary-cache service with paid plans and a self-hosted option by contact. | Not required. Do not require a Cachix account, token or paid subscription. A public cache such as `cache.nixos.org` is optional; the product must retain a local/offline path. |

### 1.1 Important inconsistency

The devenv marketing site at <https://devenv.sh/> displayed "MIT licensed" in its
footer at fetch time, while the repository `LICENSE` file and `Cargo.toml` both
say Apache-2.0, and the GitHub repository license badge says Apache-2.0. The
repository files are the authoritative distribution license. The marketing text
is a documentation inconsistency, not a dual license. The License Gate should
record Apache-2.0 and cite the repository, not the footer.

### 1.2 Nix LGPL scope

Nix is a tool, not a library linked into Factory. The LGPL permits commercial
use and distribution. The practical obligations arise if Factory distributes Nix
itself or a modified Nix binary: keep the license and notices, provide the
corresponding source and a relinkable form, and identify modifications. Merely
invoking an unmodified Nix executable as a separate process does not relicense
Factory's own source. The License Gate must still record the Nix version, source
URL, hash and whether Factory distributes it or expects it as a runtime
prerequisite.

## 2. License Gate policy

The environment compiler must enforce:

1. `allowUnfree = false` by default and in every generated environment unless an
   exact owner override exists.
2. Every selected package is recorded with name, version, source URL, hash,
   SPDX license expression and attribution. An unknown or missing license blocks
   the plan.
3. An unfree package requires an authenticated owner override bound to the exact
   package/version/plan/revision, with a reason and expiry. The override does not
   change the global `allowUnfree` value.
4. A denied or unknown license cannot be waived by a model, Lead, reviewer or
   Office click.
5. The SBOM and license inventory cover the environment itself and every
   selected package; they are frozen with the environment spec and retained as
   delivery evidence.
6. Attribution/notice text is generated and preserved. For Apache-2.0, include
   any upstream `NOTICE`; for MIT, preserve the copyright/permission text; for
   LGPL, include the license, source offer and modification notice where
   applicable.
7. No Devenv Cloud/Cachix account, token, paid plan or proprietary cache is a
   requirement. Any optional cache is labelled optional and cannot be the only
   build path.
8. Network acquisition is credential-free and goes through the egress broker;
   the builder runs without host/Factory/provider/GitHub credentials.

## 3. Commercial-use conclusion

**Nix (LGPL-2.1), nixpkgs (MIT) and devenv (Apache-2.0) can be used in a
commercial product under their respective licenses**, subject to the notice,
source/relink, modification and attribution obligations above. The combination
does not require a paid license or a mandatory hosted service. Devenv Cloud and
Cachix are optional commercial services and must not be required by the product.

This conclusion covers the three components, not the transitive package set. A
future environment may include a package with a different or incompatible
license; the License Gate must evaluate it and the owner must decide any unfree
or exceptional use.

## 4. Reproduction

The findings above were checked against:

- Nix `COPYING` (LGPL-2.1 text): <https://raw.githubusercontent.com/NixOS/nix/master/COPYING>
- nixpkgs `COPYING` (MIT text): <https://raw.githubusercontent.com/NixOS/nixpkgs/master/COPYING>
- devenv `LICENSE` (Apache-2.0 text): <https://raw.githubusercontent.com/cachix/devenv/main/LICENSE>
- devenv `Cargo.toml` (`license = "APACHE-2.0"`): <https://raw.githubusercontent.com/cachix/devenv/main/Cargo.toml>
- devenv repository license badge/README: <https://github.com/cachix/devenv>
- Devenv Cloud: <https://devenv.sh/cloud/>
- Cachix pricing: <https://www.cachix.org/pricing>

A later implementation must pin the exact Nix/devenv versions it ships and
re-check their license files at that revision. A `master`/`main` fetch is not a
version pin.

## 5. Disclaimer

**This document is not legal advice.** It records license files and a practical
commercial-use reading for engineering planning. It does not create attorney-
client privilege or replace review by qualified counsel for distribution,
modification, patent, export or jurisdiction-specific questions. If Factory
distributes Nix/devenv binaries or modifies them, obtain a legal review of the
notice/source/relink obligations before release.
