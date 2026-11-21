# Exact source evidence artifacts

`ed-1.21.1-source-proof.json` is the retained, independently verified
cache.nixos.org/store recovery for the bootstrap `ed` source fixed-output path.
It is evidence, never a licence approval.

It carries the exact source/output store and derivation paths, flat SHA-256,
NAR SRI/nix32/size, cache signature, fingerprint and CA, plus the source and
output derivation ATerms. The Core loader verifies the Ed25519 cache signature
with the pinned `cache.nixos.org-1` key, the flat/NAR/CA/size relationship and
the source-drv membership in the exact output derivation before constructing a
`SourceEvidenceProof`. The Lead review and Core decision remain separate.

Recovery provenance:
`/home/james/.local/share/software-factory/identity-facts-evidence-20261119/uat-ed-source-0fa4990e`
(flat SHA `d6d0c719...`, cache narinfo/signature, source/output ATerms).
