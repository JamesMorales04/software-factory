# Service placement assessment: optional Tailscale Serve node

| Field | Assessment |
|---|---|
| **Component** | Optional `tailscale` Compose service |
| **Responsibility** | Maintain one Tailscale node identity and privately proxy HTTPS Office traffic to Core's restricted Office listener on port 8768. It does not authenticate Factory users, make authorization decisions, or own Factory state. |
| **Proposed owner** | Deployment infrastructure; Core remains application authority. |
| **Trust classification** | Trusted infrastructure under `TRUSTED_LOCAL`; packaging does not certify isolation. |
| **Authority and credentials** | The node's private Tailscale identity key is persisted in the dedicated `factory-tailscale-state` volume. First enrollment is an interactive login URL; no auth key, Core/provider token or service secret is supplied through Compose. The volume is sensitive and must be protected by the local Podman account. |
| **Untrusted execution exposure** | No code execution. It parses untrusted network traffic and proxies HTTP requests to Core. |
| **Selected placement** | LEVEL C persistent container, opt-in overlay only. It requires a stable Tailscale node identity and its own network namespace/lifecycle. |
| **Alternatives considered** | Host-installed Tailscale would violate the no-host-runtime target. Host networking/TUN and `NET_ADMIN` add unnecessary privileges. Joining control/egress networks exposes unrelated Factory peers. A public reverse proxy or port forwarding violates private-only access. |
| **Reasons for selection** | A dedicated official client container supplies outbound tailnet connectivity and HTTPS Serve while userspace mode avoids `/dev/net/tun` and extra capabilities. |
| **Container/Pod/network boundaries** | The service joins only `factory-office-private` (internal, shared with Core only) and `factory-tailscale-uplink` (external egress, Tailscale only). It is absent from `factory-control` and `factory-egress`. Core retains only its existing loopback host port for local recovery; the restricted Office listener is not host-published. The Office listener explicitly omits Worker, Supervisor, verifier, reviewer and Quality Gate role/control routes. Tailscale publishes no host port. Serve exposes HTTPS 443 privately to the tailnet; Funnel is explicitly false. |
| **Filesystem and secret access** | Root filesystem read-only; tmpfs only for runtime files; one named writable node-state volume; one read-only bind to the static Serve configuration. No host HOME, runtime socket, Core data/config, provider secrets or other service config. Node identity state is a credential and is not included in logs or evidence. |
| **Lifecycle and failure behavior** | `unless-stopped`; node identity persists across restart. Tailscale control-plane or DERP failure removes remote availability; Core remains local when the overlay is disabled. Removal of the state volume revokes continuity and requires new enrollment. |
| **Resource constraints** | CPU is limited to 0.5, memory to 256 MiB and process count to 128. Persistent state-volume quota is not configured. Outbound Tailscale control/DERP traffic is required and is not destination-pinned. These are open hardening items. |
| **Persistent state ownership** | Tailscale container exclusively writes its node state volume. It is separate from all Factory volumes. No Factory migration or deletion path touches it. |
| **Remote Worker implications** | None. The service is an Office access path and does not enroll, dispatch or authorize Workers. |
| **Required tests** | Static policy negatives for capabilities, devices, host ports, auth material and network membership; real rootless Podman boot/login; HTTPS Serve to Core; node identity persistence after restart; verify no public Funnel and no Core-adjacent service reachability; cross-network mobile access; cleanup/revocation. |
| **Known risks** | Rootless Podman networking and interactive enrollment are unverified for this candidate; HTTPS certificate consent may require tailnet admin action; no resource or egress bounds; a compromised node has network access to Core's full API but cannot bypass Core sessions. |
| **Independent review** | PENDING |
| **Decision status** | `PROPOSED`; real authenticated tailnet and mobile-network acceptance are `NOT_TESTED`. |
