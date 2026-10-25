# Trusted-local provider boundary assessment (2026-10-24)

**Status:** Author assessment for deterministic fixtures. It is not independent
security certification, not an isolation certificate and not authorization for
live provider coding. TRUSTED_LOCAL_PROVIDER is never ISOLATED.

## Scope and assets

Assets: provider credentials, Core/Worker/Supervisor tokens, host HOME and
arbitrary host files, the Podman socket, other projects, candidate integrity and
the Quality Gate. Adversaries considered: malicious model output, a compromised
provider process, a compromised Worker, an untrusted command and a stale or
orphaned provider session.

## Findings

1. **Compromised Worker.** The Worker is a trusted role. It holds the provider
   credential and can access host resources under the owner's account. This is
   TRUSTED_LOCAL, not an isolation boundary. A compromised Worker can exfiltrate
   its credential. Mitigation requires OS/credential confinement that is not
   implemented or independently verified.
2. **Compromised provider process.** The provider server currently runs outside
   the project sandbox. A compromised provider process therefore has the host
   user's filesystem authority even though model output itself cannot execute
   tools. This is the principal blocker to claiming credential confinement for
   live provider coding. The deterministic fake server is test code only.
3. **Supervisor boundary.** The Supervisor receives no provider credential, no
   Podman socket and no host HOME. It owns the exact Attempt worktree, imports
   content into a rootless Podman named volume, and applies only validated files
   or frozen-check commands. Provider writes are path-scoped; commands must
   exactly match a frozen ActionSpec check. A denied `read_file` tool never
   executes.
4. **Source context.** The Worker sends at most 16 allowed files / 64 KiB.
   Credential-shaped names (`.env`, `.npmrc`, `.netrc`, private keys, names
   containing secret/credential/password) are excluded. The provider fixture
   asserts a synthetic `SUPER_SECRET` value never enters any prompt.
5. **Durability.** Session intent is recorded before external effects, ACTIVE
   identity before the prompt, and tool REQUEST/ALLOW/DENY/RESULT plus terminal
   usage/cleanup receipts are durable. Unresolved sessions block replacement
   dispatch. A recovery Worker deletes a confirmed orphan after Core restart.
6. **Termination.** A disconnect, abort request or idle event is not proof of
   termination. Completed sessions require session DELETE followed by GET 404.
   Cancellation/revocation mark sessions `CANCEL_REQUESTED`; they do not claim
   external termination.

## Residual risks / exact blockers

- No independent verification of host/credential confinement.
- A compromised Worker or provider process can reach host resources.
- Live provider coding is not authorized and must not be treated as certified.
- Reconnection to an existing live session is not implemented; recovery deletes
  confirmed orphans rather than resuming them.
- Same-UID pathname races outside the Supervisor's named-volume boundary are not
  fully eliminated.

## Required next work

Run the provider process inside a separately verified confinement boundary that
has no provider credential and can reach tools only through the authenticated
Supervisor broker; prove bypass negatives and independent isolation before any
ISOLATED claim. Until then, only deterministic fake-provider execution and the
explicitly unapproved live envelope may be discussed.
