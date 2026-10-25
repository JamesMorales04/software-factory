# Service placement policy

**Status:** Canonical placement policy for the native Rust branch. It defines how Factory components are assessed and where they may run. It does **not** approve a deployment, grant authority, certify isolation, or prove that a component exists.

**Applies to:** every first-party Factory component, including Core modules, Office delivery, SQLite ownership, Worker processes, provider servers, per-Attempt execution, verifier and reviewer runners, planning and assessment roles, scheduling, quality and GitHub delivery, local and remote.

Placement is a security, lifecycle and ownership decision, not a packaging preference. A component may change packaging, but its trust class, authority, credentials, persistent state and required isolation must remain explicit. Placement selection and approval are `DETERMINISTIC` policy decisions; model or skill output may supply advisory evidence, but cannot choose a trust class, approve a boundary or certify isolation.

## 1. Placement levels

| Level | Definition | Boundary and lifecycle | Permitted use | What it does not prove |
|---|---|---|---|---|
| **LEVEL A — In-process module** | Code linked into an existing process (Core or Worker); no separate OS process. | Shares address space, credentials, crash domain, lifecycle and resource limits with its host. | Trusted domain logic, policy, SQLite access, scheduling, routing, gate, static Office assets. | No failure or credential separation; not a sandbox. Untrusted code is forbidden. |
| **LEVEL B — Independent process** | Separate OS process with its own lifetime, file descriptors and crash domain; optionally a distinct OS identity. | Same host, kernel and user unless additional controls are applied; supervised and restartable. | Core, Worker supervisor, provider servers, verifier and reviewer runners, delivery broker, standalone Office development server. | Not a security boundary against same-user compromise; not a container or VM; no isolation claim. |
| **LEVEL C — Persistent container** | Long-lived container with its own filesystem view, network identity and restart policy. | Stateful service lifecycle; may share a Pod or network only with explicitly trusted, same-owner services. | Persistent auxiliary services that genuinely need packaging, restart or network identity. | A container or Pod alone is not certified isolation; co-location weakens boundaries. |
| **LEVEL D — Ephemeral execution container** | Disposable container created for one Attempt or bounded work unit to run untrusted code. | Scoped workspace, bounded CPU/RAM/PIDs/time/disk, no secrets, explicit egress; reconciled and removed. | Repository commands, tests and builds, agent tool calls, candidate execution, downloaded skill scripts. | Rootless Podman or OCI is a candidate runtime, not certified isolation; no proof of credential confinement or kernel separation. |
| **LEVEL E — Stronger isolation** | A boundary beyond a single shared-kernel container: VM or microVM, dedicated verified host or user separation, or an independently verified remote isolated Worker. | Stronger namespace, credential and failure separation, tested per topology and image/runtime digest. | Hostile or multi-tenant code, credential-bearing untrusted work, cases where D is demonstrably insufficient. | Never inferred from remote location, architecture or one fixture PASS; requires candidate-bound evidence. |

**Minimum for untrusted execution:** LEVEL D or E. LEVEL A, B and C are for trusted code or components operating under explicit trust acceptance. They may ingest untrusted data, but they must not execute untrusted code. A Git worktree, same-UID process or shared container is not a sandbox.

## 2. Eight decision rules

1. **Place by trust, authority and blast radius, not by convenience.** The boundary must separate what must not be compromised together: authority, credentials, authoritative state, untrusted code and other projects. A boundary that does not separate these is packaging only.
2. **Core authority remains in Core.** Authorization, risk, budgets, leases, candidate identity, evidence, gate and delivery credentials stay in the Core trust domain. Workers, provider servers, models, skills and containers never receive Core authority or delivery rights.
3. **Untrusted execution is LEVEL D or E only.** Repository commands, candidate tests and builds, agent tool calls, downloaded skill scripts and any code influenced by untrusted input run only in disposable, resource-bounded, credential-free execution.
4. **Credentials are scoped, separated and brokered.** Each boundary receives only the credentials needed for its own responsibility. Provider credentials, GitHub tokens and host secrets never enter an untrusted workload. Prefer short-lived, least-privilege, mediated access and keep credential-bearing servers outside execution containers.
5. **Persistent state has one owner and one writer.** Authoritative state is owned by Core and its SQLite database and accessed through typed APIs and transactions. No other component, provider, agent container or remote Worker reads or writes the database or its files directly. Derived caches are disposable and provenance-marked.
6. **Use LEVEL B when lifecycle, failure, identity or blocking work must be contained.** A separate process is justified by independent restart or crash isolation, a distinct credential or identity, an external protocol, or keeping blocking or CPU-heavy work out of Core. Do not add a process only for symmetry.
7. **LEVEL C is persistent packaging, not a trust upgrade.** Use it for a long-lived service that needs its own runtime, network identity or restart policy. Shared containers and Pods follow the rules below; a Pod is not a security boundary, and co-location never authorizes credential or workspace sharing.
8. **LEVEL D is per-Attempt and fail-closed; LEVEL E needs independent evidence.** Every D instance is disposable, bounded, reconciled and isolated from other Attempts. If required isolation or credential confinement is unavailable, dispatch is `BLOCKED` and must not fall back to the host. E is justified only by a demonstrated threat D cannot contain and remains `NOT_CERTIFIED` until candidate-bound independent verification.

## 3. Shared-container and Pod rules

- A Pod is a scheduling and network unit, not a trust boundary. Containers in one Pod share a network namespace by default; shared PID or IPC namespaces only increase coupling.
- Untrusted execution must not share a container, Pod, network namespace, process namespace, writable volume, credential set or service identity with Core, SQLite, provider servers, GitHub delivery, the Worker supervisor, or another project's workspace.
- Same-Pod trusted services must have the same trust class and owner, explicit per-container identities, and no shared writable volume across trust classes. Read-only mounts are preferred.
- No privileged containers, host PID/network/IPC, `hostPath` mounts, host HOME, container-runtime sockets, device passthrough, cloud metadata routes or service-account tokens for LEVEL D or E workloads.
- Network is default-deny with explicit egress. Untrusted execution must not reach Core or database ports, provider control ports, the host LAN or metadata services. Persistent services expose only their intended authenticated endpoint.
- Do not expose the Podman or Docker socket to a workload. A component that needs nested container execution requires a brokered, scoped launcher or LEVEL E; a socket mount is not acceptable.
- Cross-Attempt reuse is forbidden: no shared mutable workspace, credential-bearing cache, container name or writable layer. Read-only, digest-pinned images and fixture caches may be shared only when they contain no secrets and are integrity-checked.
- If a credential-bearing provider server and untrusted tool execution cannot be separated into B/C plus D/E, the placement is `BLOCKED` for that work. Running provider CLI and untrusted shell in the same credential-bearing sandbox is evidence against secret protection, not for it.
- Every container and Pod has a durable owner record, bounded lifetime, stable identity and orphan reconciliation. Deleting a Pod is not proof that an external provider session stopped.

## 4. Mandatory placement questions

Every assessment must answer all of these; an unanswered question is an open risk, not an implied default.

1. **Responsibility and authority.** What does the component own, and what authority does it exercise or consume?
2. **Trust classification.** Is it trusted control plane, trusted infrastructure, an independent role, or untrusted execution? Which inputs and outputs are untrusted?
3. **Credentials and external effects.** Which tokens, keys, provider credentials, GitHub rights, host resources or external effects does it need? Can they be scoped, brokered or short-lived?
4. **Untrusted execution exposure.** Does it execute repository code, scripts, tests, tools or model-directed actions? If yes, why is it not LEVEL D or E?
5. **Blast radius.** If the component is compromised, what authority, credentials, state, other projects and external effects are reachable?
6. **Persistent state ownership.** What durable state does it own, who may write it, and how are migration, backup and deletion controlled?
7. **Lifecycle and failure behavior.** How does it start, stop, restart, cancel, time out, clean up and reconcile orphans? What is left after a crash?
8. **Resource constraints.** What are its CPU, memory, PID, time, disk, network and concurrency bounds, and what happens at each limit?
9. **Remote Worker implications.** Does the placement hold for a remote Worker? How are authentication, transport, local state, credential provisioning and isolation handled without direct database access?
10. **Required tests and evidence.** Which negative boundary tests, cleanup tests and independent checks prove the claim? What remains `NOT_CERTIFIED`, `BLOCKED` or `NOT_TESTED`?

## 5. Security invariants

1. Untrusted workloads must not receive provider credentials, GitHub tokens, Core API or authority tokens, the Core database or its volume, host HOME, host filesystem paths, a privileged Podman or Docker socket, host namespaces or devices, cloud metadata, service-account tokens, or another project's data.
2. Provider and delivery credentials remain in the trusted control plane, or in a dedicated broker. A provider server may hold only its own provider credentials; it must not receive GitHub or Core tokens.
3. Rootless Podman is a candidate LEVEL D runtime. It is **not certified isolation** and does not by itself prove secret protection, UID separation, mount safety, kernel confinement or cleanup. The sandbox mount no longer uses a host pathname (named volume snapshot), but the unverified provider-in-sandbox/credential path remains a blocker. Rootless Podman is still not certified isolation.
4. There is no silent host fallback. If required isolation, credential confinement, authorization or budget is unavailable, the action is `BLOCKED`. Never downgrade to host execution or TRUSTED_LOCAL without explicit owner authorization recorded with actor, reason and scope.
5. TRUSTED_LOCAL is broad local access under accepted risk. It is never isolated, sandboxed, confined or certified, and must not be described as such.
6. Git worktrees isolate revisions, not processes, credentials or the kernel. A clean worktree is not a sandbox.
7. All provider, web, tool, skill and model output is untrusted. It cannot grant authority, choose its own placement, expand capabilities or certify isolation.
8. Persistent authoritative state is written only by its owner. Cross-boundary access uses typed, authenticated APIs; no shared mutable database file or volume.
9. Egress is explicit and bounded. Untrusted execution cannot reach Core, provider control ports, the host LAN or metadata services.
10. Every security-relevant placement claim needs candidate-bound tests and independent review. Unknown or untested confinement is `NOT_CERTIFIED` or `BLOCKED`; a green fixture, architecture diagram or documentation statement is not certification.
11. Secrets and raw prompts are redacted at telemetry boundaries. No secret belongs in logs, evidence, error messages or assessment documents.

## 6. Remote Worker implications

A remote Worker is not automatically LEVEL E. Remote location does not prove isolation, credential confinement or trustworthy administration. Every remote placement must still answer the mandatory questions and must:

- authenticate to Core over an authenticated transport and receive only fenced assignments;
- never access the Core database or shared local paths directly;
- keep its own durable state and reconciliation for external sessions, containers and workspaces;
- treat credential provisioning and untrusted execution as separate decisions, with provider and GitHub credentials outside untrusted workloads;
- define behavior for network partition, clock skew, stale leases, revoked credentials and cleanup while disconnected;
- prove its actual boundary independently for the exact host, runtime and image digest. Otherwise the remote placement is `NOT_CERTIFIED`.

## 7. Initial placement assessment

The table below is the **initial baseline hypothesis** for the native Rust branch. It is not an approval, implementation claim or security certification. Most components are not implemented; see [native/README.md](../../native/README.md) and the [native handoff](../implementation/native-rust-handoff.md). No row is `ACCEPTED`.

Status vocabulary:

- `HYPOTHESIS` — initial baseline; not approved, implemented or certified.
- `NOT_CERTIFIED` — no candidate-bound independent evidence that the claimed boundary holds.
- `BLOCKED` — required isolation, credential confinement or authority is absent; dispatch must fail closed.

Implementation status is tracked separately; a placement row does not claim that Rust behavior exists.

| Component | Proposed owner | Baseline placement | Trust and exposure | Authority and credentials | Rationale and alternatives | Status |
|---|---|---|---|---|---|---|
| **Factory Core** | Core | B — trusted host process; C only as packaging | Trusted control plane; no direct untrusted execution | Owns authorization, risk, budgets, leases, evidence, gate and delivery; holds Core tokens, database access and the delivery token | A would place provider or agent code in the authority process. C does not add a trust boundary for a trusted modular monolith. Keep one authoritative Core. | `HYPOTHESIS` |
| **Factory Office** | Office | A — static assets served by Core; B standalone development server | Trusted presentation; no Factory authority; renders untrusted content | User session only; no database, Worker, provider or delivery credentials | The browser is an external client outside the Factory trust zone; LEVEL A refers to asset delivery. A avoids a second production service. Office never writes SQLite. | `HYPOTHESIS` |
| **SQLite** | Core | A — embedded library in Core | Trusted authoritative data; no untrusted access | Core-only file access and transactions | A separate database process adds operations without fixing a demonstrated boundary at current scale. The database file or volume is never mounted into provider or execution containers. | `HYPOTHESIS` |
| **Worker Supervisor / Sandbox Launcher** | Worker | B — trusted Worker process on the Worker host | Trusted infrastructure; launches untrusted code but remains outside it | Worker session credential; provider and GitHub credentials withheld by default; any brokered provisioning needs independent verification | A would merge the launcher into Core and lose host separation; C is packaging. The supervisor must remain outside per-Attempt containers and must reconcile orphaned execution. | `HYPOTHESIS` |
| **Codex App Server** | Provider adapter | B — credential-bearing provider process outside D | Trusted infrastructure with provider credentials; model and tool output untrusted | Provider credentials only; no GitHub, Core or delivery tokens | C may package a long-lived server but is not certified. A inside Worker would not contain crashes. Co-locating provider credentials with untrusted shell is forbidden; tool execution must be mediated into D. | `HYPOTHESIS / NOT_CERTIFIED` |
| **OpenCode Server** | Provider adapter | B confined by bubblewrap user/pid/net/ipc/uts/cgroup namespaces; no host HOME/repository/runtime socket or external network | Trusted infrastructure with only a per-run server capability; session and tool output untrusted | Per-run bridge capability; long-lived provider credential remains in the trusted Worker; no Core, Supervisor or GitHub tokens | C for persistent packaging is possible but uncertified. Optional model egress uses a per-run authenticated broker pinned to one public upstream; tool execution is mediated into D. Bubblewrap is a candidate boundary, not certified isolation. | `HYPOTHESIS / NOT_CERTIFIED / BLOCKED_FOR_LIVE_EGRESS` |
| **Per-Attempt execution sandbox** | Worker | D — ephemeral container per Attempt or bounded unit; E if the threat requires | Untrusted; arbitrary repository, agent or tool code | None: no provider, GitHub or Core tokens; no host HOME, runtime socket, database or host paths | A, B and C are forbidden for untrusted code. Rootless Podman is the current candidate, not certified. The named-volume mount avoids host pathname binding, but provider-in-sandbox/credential confinement remains unresolved, so credentialed untrusted execution is blocked. | `HYPOTHESIS / NOT_CERTIFIED / BLOCKED` |
| **Verifier execution** | Independent verifier | B control process plus D for candidate code and tests | Independent role; the candidate is untrusted | Verifier token only; no implementer, reviewer or delivery credentials; read-only candidate | A would put verification inside Core or the implementer identity. Candidate builds and tests must run in D with a read-only source mount. Evidence is immutable and exact-candidate bound. | `HYPOTHESIS / NOT_CERTIFIED` |
| **Reviewer execution** | Independent reviewer | B — read-only reviewer process or session; provider inference via B | Independent role; reads untrusted code and evidence without executing it | Reviewer token only; no implementer, verifier or delivery credentials; read-only candidate | A would collapse independence into Core. No D is needed when the reviewer does not execute candidate code; if it must execute anything, that part is D. Model review remains advisory. | `HYPOTHESIS` |
| **Intelligent Lead** | Core plus provider adapter | A deterministic validation in Core plus B bounded model session | Advisory model output; no authority | No delivery or provider-administration credentials; scoped model calls only | A long-lived Lead service is unnecessary. Reuse Core context and a bounded provider session; plan validation and freezing stay deterministic in Core. The role is not activated. | `HYPOTHESIS` |
| **Model Router** | Core | A — in-process Core module | Trusted deterministic control; no untrusted execution | No separate credentials; adapters use scoped provider access | A separate process would create a second routing authority. Keep one router and freeze the selected route per Run. Provider health probes are adapter calls, not authority. | `HYPOTHESIS` |
| **Knowledge Resolver** | Core | A — in-process module with read-only adapters | Trusted logic; retrieved content untrusted | Scoped read-only knowledge access if any; no provider, GitHub or delivery tokens | B is justified only by isolation or measured scale. Do not create a privileged agent. Treat retrieved text as data, never as instructions or authority. | `HYPOTHESIS` |
| **Skill Registry** | Core | A control plane; D or E for skill scripts and admission execution | Registry trusted; external skills untrusted | Registry metadata in Core; no skill receives Core, GitHub or provider credentials | A separate registry service adds no trust boundary. Admission and script execution are credential-free execution. Global promotion needs an independent decision and revocation. | `HYPOTHESIS / NOT_CERTIFIED` |
| **Semantic Assessment** | Core plus provider adapter | A validation and orchestration plus B bounded model session | Hybrid mechanism; model output advisory and untrusted | Scoped model access only; no Core authority or delivery credentials | No dedicated long-lived agent; reuse existing sufficient context. If an assessment executes tools, that execution is D, not A or B. Core validates the result. | `HYPOTHESIS` |
| **WorkGraph scheduler** | Core | A — single in-process Core scheduler | Trusted deterministic control; validated plan input | No credentials; dispatch authority remains Core-owned | B or HA only after measured need and a single-writer design. A second scheduler is forbidden. Scheduling decisions are deterministic, not model-selected. | `HYPOTHESIS` |
| **Quality Gate** | Core | A — in-process Core decision | Trusted deterministic policy | Core database only; no delivery credentials | A separate gate service would duplicate authority. Receipts and evidence are untrusted input and must be validated. The decision is immutable and transactional. | `HYPOTHESIS` |
| **GitHub delivery** | Core | A — in-process delivery adapter with outbound HTTPS; B broker as a hardening option | Protected external effect; candidate content untrusted | GitHub token confined to Core; never Worker, model, skill or execution container | A keeps delivery inside Core authority. A dedicated broker could reduce token exposure later but needs independent review. Authorization to execute does not authorize delivery. | `HYPOTHESIS` |

## 8. Assessment and approval

- Use the [service placement assessment template](../templates/service-placement-assessment.md) for every new or materially changed component.
- Select the lowest level that satisfies the trust, authority, state, lifecycle, resource and remote requirements; record why higher levels were rejected. Do not choose a lower level merely to avoid operational work.
- Security-relevant placements require independent review. Protected changes to authority, credentials, sandbox policy, quality policy or delivery require the governing ADR and owner approval, not an ordinary documentation change.
- Reassess when trust classification, credentials, state ownership, provider/runtime, container topology, network exposure, lifecycle or remote scope changes.
- A placement decision is not evidence that the component works. Required tests, independent verification and candidate-bound evidence are separate obligations.
- Update this policy, the affected ADR, tests and the assessment together. Do not leave a stale baseline in place.

## 9. Non-certification

Nothing in this document certifies isolation. Rootless Podman fixture results, architecture diagrams, unit tests, protocol probes and TRUSTED_LOCAL runs are not isolation certification. `NOT_CERTIFIED` and `BLOCKED` remain until independent, candidate-bound evidence exists for the exact topology, runtime and image digest.

## References

- [ADR-0018 — Native Rust replacement](decisions/ADR-0018-native-rust-replacement.md)
- [Isolation, credentials and trust boundaries](../security/isolation.md)
- [Trusted local execution](../security/trusted-local.md)
- [Rust engineering standards](../engineering/rust-standards.md)
- [Service placement assessment template](../templates/service-placement-assessment.md)
- [Service placement skill](../../.agents/skills/service-placement/SKILL.md)
