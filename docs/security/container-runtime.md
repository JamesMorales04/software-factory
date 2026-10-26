# Container runtime trust boundaries

**Status:** developer-preview `TRUSTED_LOCAL` implementation, author-run.
Not SEC-01 ISOLATED, not independently certified, not a deployment or Windows
claim.

## Measured phase-zero environment (2026-10-26)

| Item | Value |
|---|---|
| Host | CachyOS, Linux `7.3.0-3-cachyos`, x86_64 |
| Outer engine | rootless Podman `6.1.4`, crun, cgroup v2 |
| SELinux | disabled on the test host |
| AppArmor | disabled on the test host |
| `/dev/fuse` | present, but not used by the engine (VFS storage) |
| User namespaces | `kernel.unprivileged_userns_clone=1` |
| Inner engine | `quay.io/podman/stable:v5.8.9`, private VFS storage, cgroupfs |
| Runtime capabilities | `SYS_ADMIN`, `NET_ADMIN`, plus the normal root-process set; `no-new-privileges`; never privileged |
| Host socket | not mounted or exposed |
| Child network | `--network=none`; `wget` to `1.1.1.1` reported unreachable |
| Child limits | Verified memory and PID cgroup files; product values 1 GiB / 128 PIDs / 2 CPUs |

Phase zero created at least two independent child containers, distinct named
volumes, a network-disabled verifier child, bounded cgroups, and verified
cleanup. A controlled engine restart preserved engine storage; the broker
removes managed containers on startup and the Supervisor reconciles stale
workspace state.

A nested **rootless** engine inside the rootless outer container failed for
child ID mapping:

```text
newuidmap: write to uid_map failed: Operation not permitted
```

The implemented engine is rootful inside the outer rootless user namespace.
This is the exact reason it is not described as rootless-in-rootless.

## Boundaries

| Boundary | What crosses | What must not cross |
|---|---|---|
| Host → outer runtime container | Namespaced capabilities, private engine volume, provider-session volume, bundle/config volumes | Host Podman socket, host HOME, host network namespace |
| Broker → inner engine | Typed operations only | Arbitrary Podman arguments, host paths, unlisted images |
| Inner engine → child | Allowlisted image, one workspace volume or provider session subdir, bounded cgroups, no network | Core DB, provider credential, operator/role tokens, host repositories, other workspaces |
| Worker → provider container | Per-run egress token and non-secret OpenCode config; private session volume | Long-lived provider credential (held by the Worker egress broker) |
| Core → model provider | Core-held credential, budget reservation, strict schema | Provider credential to browser/Worker/sandbox |
| Compose → Office | Authenticated operator API | Unauthenticated admin endpoint |

## Required capability exceptions

The runtime container is deliberately not privileged. These exceptions are
required and reviewed together:

1. `CAP_SYS_ADMIN` — create child mount/network/user namespaces, mount cgroup2
   and run the nested engine.
2. `CAP_NET_ADMIN` — configure child loopback and the engine's private
   networking; the engine has no host network namespace.
3. cgroup2 remount read-write **inside the runtime container's cgroup
   namespace** — enforce child CPU/memory/PID limits. The engine does not gain
   host cgroup authority.
4. VFS storage — avoids `/dev/fuse` and overlay-on-overlay; slower and larger.
5. No `--privileged`, no host Podman socket, no host network, no `/dev/fuse`
   device grant.

These are not silently enabled: they are visible in `compose.yaml` and are a
security review gate. A child-to-engine escape would have the runtime
container's namespaced capabilities and access to all engine volumes; this is
why the engine is trusted infrastructure and why SEC-01 remains blocked.

## Provider credential path

- Core stores provider credentials as AES-256-GCM ciphertext under a master key
  in the Core-only config volume.
- The API masks them after submission; there is no read-back.
- The Worker fetches only the assigned provider profile over the internal
  control network with its worker session token, then holds the credential in
  memory inside the loopback egress broker.
- The provider container receives only a per-run egress token, the private
  session directory and non-secret configuration. It has no network namespace
  access to Core or the Worker.
- Revocation clears the ciphertext, disables the route and is audited.

## Negative and cleanup evidence

- Child network-disabled probe: unreachable.
- Named-volume-only mount; no host path in the child specification.
- Child cgroup limits read back from inside the child.
- Parallel attempts produced separate candidates and worktrees.
- The broker prunes managed containers at startup; the Supervisor removes stale
  workspaces on reconciliation.
- Provider session cleanup returned `container_removed_verified_absent` in the
  fixture journey.

## Not claimed

- SEC-01 ISOLATED certification, kernel confinement or credential confinement.
- SELinux/AppArmor-labeled hosts or Windows/Podman Machine.
- Cross-project filesystem isolation after engine/broker compromise.
- Paid live provider UAT, live Codex container transport or ChatGPT OAuth.
- Independent security review. A dedicated DeepSeek security review remains a
  merge gate.
