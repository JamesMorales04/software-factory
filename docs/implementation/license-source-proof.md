# Exact source proof and plan STALE for the ed decision

Status: integration evidence/coordination; no runtime activation.

## Exact source proof

The retained identity-facts receipt has the exact ed **output** fact but no
**source** fact. The repaired contract accepts a separate verified source
proof. Candidate proof from the recovered cache/source evidence:

`/home/james/.local/share/software-factory/identity-facts-evidence-20261119/uat-ed-source-0fa4990e/source-evidence-proof-candidate.json`

- sourceStorePath `/nix/store/wa88…-ed-1.21.1.tar.lz`
- sourceDrvPath `/nix/store/j151…-ed-1.21.1.tar.lz.drv`
- flatSha256 `d6d0c7192b02b0519c902a93719053e865ade5a784a3b327d93d888457b23c4b`
- narSri `sha256-dEsPDHFVSahejFEnqz5gAMxx+ls+idLXQ6qOovYOecE=`
- narNix32 `1hbr1vva53ma8gbx529ybgx73k00c0zan9siiigahjamf460yjvl`
- narSize `70136`
- signature `cache.nixos.org-1:xYwC8Vo2buDIPQNzCyHhC8Kz7Y6riU596Zp3tnRTIicdLtih5xQzAPlip25FtXIsShiTfkl4eABEg4TX0c+ZDA==`
- fingerprint `1;/nix/store/wa88…tar.lz;sha256:1hbr…;70136;`
- ca `fixed:sha256:0jrwn9bq921xv4kv78w4lzjssrg8af8734raj2f53c025ccwgl6n`
- outputDrvPath `frkd…`, outputStorePath `36qz…`
- sourceInOutputInputDrvs true; signatureValid true

All values are derived from the exact asset/cache/pathinfo and our own verified
copy, not fabricated. The broker receipt still supplies the output fact; the
source proof is separate, as agreed with Core98/Reviewer70.

## Plan STALE

The retained plan `c6cb4499…` carries
`licenseInventory.catalogSha256 = 725ba589…`, while the current policy catalogue
is `edef68e1…`. The repaired `from_plan` correctly rejects it with
`PolicyMismatch`; this is a real contract STALE, not a decision.

Resolution after the backend/build is ready, without mutating the frozen plan:

1. Recompile a new environment plan revision for the same intake with the
   current catalogue/profile and real inventory.
2. Re-run the Lead environment review only if the frozen review cannot be
   reused or rebound; do not force the old review onto the new plan.
3. Recompute the plan/ inventory/evidence hashes; bind the decision to the new
   stage; the builder recomputes the exact `{"entries": …}` bundle and marks
   STALE on any mismatch.
4. If the recompile requires a new Lead-route model call, it is a separate,
   authorized call; no provider call is made for the source metadata itself.

No decision is executed until the new plan stage exists. The decision endpoint
must surface `LICENSE_PLAN_STALE`/blocked, not accept the old plan or the static
asset.

## Runtime source-proof artifact

The exact recovery is retained at
`deploy/evidence/ed-1.21.1-source-proof.json` (see
`deploy/evidence/README.md`). It carries the source/output store and
derivation paths, flat SHA-256, NAR SRI/nix32/size, cache signature,
fingerprint, CA and the source/output derivation ATerms.

The Core loader verifies the Ed25519 `cache.nixos.org-1` signature over the
exact fingerprint, the flat/NAR/CA/size relationship and the source-drv
membership in the exact output derivation before constructing a
`SourceEvidenceProof`. The artifact is exact recovery evidence, not a licence
approval; the Lead review and Core decision remain separate. The decision
persists only `sourceEvidenceSha256`; the builder recomputes the same object
from the signed identity asset and requires the digest.
