# Service placement assessment: `<Component>`

Use this template with the [service placement policy](../architecture/service-placement-policy.md). Keep it concise, name unresolved facts explicitly, and do not claim a boundary or certification that has not been independently tested.

| Field | Assessment |
|---|---|
| **Component** | `<component name>` |
| **Responsibility** | `<what it owns and does not own>` |
| **Proposed owner** | `<Core / Worker / Office / provider adapter / independent role / ...>` |
| **Trust classification** | `<trusted control plane / trusted infrastructure / independent role / untrusted execution>` |
| **Authority and credentials** | `<authority exercised or consumed; credentials, tokens, scopes, expiry and brokering>` |
| **Untrusted execution exposure** | `<none / untrusted input only / executes untrusted code; which code and under what influence>` |
| **Selected placement** | `<LEVEL A / B / C / D / E, with conditions and rationale>` |
| **Alternatives considered** | `<realistic alternatives, including the simpler option, and why each was rejected>` |
| **Reasons for selection** | `<trust, authority, state, lifecycle, failure, resource, remote and maintenance reasons>` |
| **Container/Pod/network boundaries** | `<container/Pod membership, namespaces, network exposure, egress, lateral access, shared services>` |
| **Filesystem and secret access** | `<mounts, read/write scope, host paths, HOME, sockets, database access, secret provisioning and redaction>` |
| **Lifecycle and failure behavior** | `<start/stop/restart/cancel/timeout, crash behavior, cleanup, orphan reconciliation, unresolved external state>` |
| **Resource constraints** | `<CPU, memory, PIDs, time, disk, network, concurrency and behavior at each limit>` |
| **Persistent state ownership** | `<owner, single writer, schema/migrations, backup, deletion, derived caches>` |
| **Remote Worker implications** | `<authentication, transport, local state, credential provisioning, isolation, partition and stale-lease behavior>` |
| **Required tests** | `<negative boundary, credential, path, network, lifecycle, cleanup, restart, concurrency, independent acceptance>` |
| **Known risks** | `<unresolved races, unverified paths, residual exposure, open assumptions>` |
| **Independent review** | `<reviewer and disposition, or PENDING; separate from the author>` |
| **Decision status** | `HYPOTHESIS / PROPOSED / ACCEPTED / REJECTED / SUPERSEDED` + evidence reference |

**Decision status values:** `HYPOTHESIS` is an unapproved baseline. `PROPOSED` is under review. `ACCEPTED` requires the applicable authority and evidence; it is not a security certification. `REJECTED` and `SUPERSEDED` require a reason and replacement reference.
