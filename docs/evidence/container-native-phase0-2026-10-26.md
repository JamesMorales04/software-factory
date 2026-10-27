# Container-native phase-zero and deterministic Compose evidence (2026-10-26)

**Status:** author-run developer-preview evidence. Not independent acceptance,
not paid UAT, not SEC-01 isolation and not a Windows/deployment claim.

## Environment

| Item | Measured value |
|---|---|
| Host | CachyOS Linux, `7.3.0-3-cachyos`, x86_64 |
| Podman | `6.1.4`, rootless, crun, cgroup v2 |
| SELinux / AppArmor | disabled / disabled on this host |
| `/dev/fuse` | present; not used (VFS) |
| Nested engine | `quay.io/podman/stable:v5.8.9`, private VFS, cgroupfs |
| Outer engine caps | `SYS_ADMIN`, `NET_ADMIN` plus normal root-process caps; no privileged; no host socket |

## Phase zero

The spike first tested a nested rootless-in-rootless engine. `newuidmap`
failed with `Operation not permitted` when mapping child IDs. The selected
topology is a private rootful engine inside the outer rootless user namespace.

Working spike results:

- created two independently named child containers;
- distinct named volumes;
- verifier child with `--network=none`; an HTTP probe to `1.1.1.1` failed
  (`Network unreachable`);
- cgroup v2 memory.max and pids.max read back inside the child;
- no host Podman socket, no privileged mode, no host network and no host HOME;
- managed-container reconciliation and named-volume cleanup;
- engine storage survived a controlled engine restart.

The exact capability/remount exceptions and residual risks are recorded in
[container runtime security](../security/container-runtime.md).

## Deterministic Compose acceptance

Command (tested Compose provider on this host):

```sh
PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose bash scripts/compose-e2e.sh
```

The harness:

1. creates a disposable Git fixture served over local HTTP;
2. starts the clean Compose stack with fresh volumes using one Compose command;
3. completes first-run enrollment and invalidates the code;
4. imports the Git project into managed storage;
5. configures allowed paths/capabilities and freezes an Action with functional
   and negative checks;
6. creates a scoped delegation, authorizes a Campaign and dispatches an Attempt;
7. waits for a real nested provider-container candidate, independent verifier,
   reviewer and `ACCEPTED` gate;
8. runs `compose down` and `up` without deleting volumes and verifies Project,
   Campaign and gate persistence.

Final author-run result after independent-review remediation on implementation
commit `889f601b46ec03ff28a2c4b9eb26ea17dabe6116`:

```text
status: PASS
candidate: d23d48de-fa8c-4466-bebd-e4c833dd9801
attempt: 4d50e21f-be8f-41a7-b105-b0dc71eec487
gate: ACCEPTED
restart_persistence: true
paid_inference: false
```

Machine-readable evidence:
[container-native-compose-2026-10-26.json](container-native-compose-2026-10-26.json).

The provider used the deterministic fixture adapter inside the real provider
container and shared session volume; no paid inference was made.

## Browser evidence

- `scripts/compose-onboarding-smoke.mjs`: real Chromium first-run enrollment,
  Settings, HTTPS Git project registration, project configuration, typed
  Action creation and delegation creation. Zero console errors and zero 404
  responses on the tested run.
- `scripts/compose-office-smoke.mjs`: Office login, Settings provider cards,
  project configuration, Action creation and Delegations navigation with zero
  console errors/404s.
- `scripts/compose-provider-smoke.mjs`: provider credential save, reload,
  masked/configured state, model-list connection test and revocation. A dummy
  credential was used; **no inference** was started; the credential was
  revoked.

Screenshots and browser profiles are not committed.

## Parallel evidence

After the independent-review fixes, two Workers produced two concurrent
fixture attempts. Provider containers started 24 ms apart
(`00:41:52.481`/`.505` UTC), and candidate acceptance timestamps were within
the same second. Attempts `d2c19210…` / `151ac11b…` produced candidates
`2db6e3d8…` / `a6908ba7…`, and both reached independent verifier/reviewer PASS
and deterministic `ACCEPTED` gates. This demonstrates container/workspace
independence for two authorized attempts, not multi-unit fan-in integration
and not paid parallel provider execution.

## Defects found and repaired in this iteration

- initial Compose volume ownership race for `/projects` (fixed with owned mount
  points and init convergence);
- first-run setup state did not clear after enrollment (fixed);
- provider/engine JSON casing mismatch (fixed);
- runtime storage missing basic process capabilities (fixed);
- provider container stdin/volume remote limitations (avoided by co-locating
  broker with local engine CLI; bundle import uses a stable engine-local copy);
- provider bundle startup race (atomic marker + runtime wait);
- service startup before runtime readiness (bounded curl waits);
- DraftCheck UI leakage into Core JSON (fixed);
- favicon 404 (fixed with inline data URI).

## Independent review

An independent same-model security/architecture review of the first candidate
returned findings. Rootless outer-namespace enforcement, container TurnBinder
origin validation, semantic-review authority, provider-profile lease binding,
worker-scoped recovery, provider lifetime reaping, turn-release binding,
master-key permissions, static Compose policy and the destructive-project guard
were remediated and tested. Residual risks (shared provider-session volume and
UID, total volume quota, mount-path TOCTOU, trusted role receipts and
read-write verifier project volume, manual-only real E2E CI) are recorded in
[container-native status](../implementation/container-native-status.md) and the
[security record](../security/container-runtime.md). They are not claimed
fixed.

## Not tested / blocked

Paid DeepSeek/OpenCode UAT of the new stack; live OpenCode-in-container
inference; Codex API-key live inference; containerized Codex stdio; native
ChatGPT OAuth; semantic Reviewer live call; multi-unit fan-in with a real
provider; SEC-01 isolation; SELinux/AppArmor hosts; Windows/Podman Machine;
independent security review; human acceptance.
