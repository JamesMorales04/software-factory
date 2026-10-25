# Trusted-local provider boundary assessment (2026-10-24)

**Status:** Author assessment for the confined deterministic provider path. It
is not independent security certification, not an isolation certificate and not
authorization for live provider coding. TRUSTED_LOCAL_PROVIDER is never
ISOLATED.

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
2. **Compromised provider process.** The deterministic provider now runs in a
   bubblewrap user/pid/net/ipc/uts/cgroup namespace with an empty HOME, a
   read-only system image, only its private 0700 session directory writable, no
   host repository, no host HOME, no `/run/user` runtime socket and no external
   network namespace. The Worker reaches it through a loopback bridge requiring
   a per-run Basic-auth capability. The long-lived provider password is not
   placed in the sandbox. This is a candidate boundary, not certified isolation.
   A real OpenCode model process cannot yet reach model egress through this
   sandbox, so live provider coding remains fail-closed.
3. **Supervisor boundary.** The Supervisor receives no provider credential, no
   Podman socket and no host HOME. It owns the exact Attempt worktree, imports
   content into a rootless Podman named volume, and applies only validated files
   or frozen-check commands. Allocation requires a Core-signed grant bound to
   the exact Attempt, worker, generation, repository, base commit and ActionSpec
   digest. Workspace operations require a per-Attempt capability bound to the
   workspace and attempt; the broad Supervisor admin bearer is not accepted.
   Provider writes are path-scoped; commands must exactly match a frozen
   ActionSpec check. A denied `read_file` tool never executes.
4. **Source context.** The Worker sends at most 16 allowed files / 64 KiB.
   Credential-shaped names (`.env`, `.npmrc`, `.netrc`, private keys, names
   containing secret/credential/password) and files whose ordinary content
   matches credential patterns (`api_key`, `token`, `secret`, `password`,
   `authorization:`, private-key markers) are excluded. The provider fixture
   asserts synthetic secrets in both `.env` and an ordinary `config.txt` never
   enter any prompt.
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
- The trusted Worker still holds the provider credential and can reach host
  resources; it is not an isolation boundary.
- The provider session directory is a writable host bind mount; its size is not
  quota-bounded, so a compromised provider could exhaust host disk.
- Sandboxed-session recovery after a Worker crash is not implemented; recovery
  currently handles explicitly unconfined fixture sessions and confirmed
  external orphans.
- Live provider model egress through the sandbox is not implemented; live
  provider coding is unapproved and fail-closed.
- Same-UID pathname races outside the Supervisor's named-volume boundary are not
  fully eliminated.
- Bubblewrap and rootless Podman are candidate runtimes, not certified
  isolation. Independent review is outstanding.

## Required next work

Run the provider process inside a separately verified confinement boundary that
has no provider credential and can reach tools only through the authenticated
Supervisor broker; prove bypass negatives and independent isolation before any
ISOLATED claim. The current bwrap candidate provides host/network/credential
negatives but is not independently certified. Until then, only deterministic
fake-provider execution and the explicitly unapproved live envelope may be
discussed.
