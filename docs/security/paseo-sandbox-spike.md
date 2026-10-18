# SEC-01 — Paseo SDK, sandbox, credentials and telemetry feasibility spike

**Status:** partial feasibility results; **PAUSED / BLOCKED, NOT CERTIFIED** by product-owner sequencing decision (2026-10-18). Read the [recorded results](SEC-01-results-2026-10-18.md). **Safety:** never expose real authentication materials to untrusted spike code. **Scope:** local CachyOS/Linux, a real Paseo daemon, `@getpaseo/client`, rootless Podman and disposable fixtures.

## Current disposition

Podman host boundaries and a functional Paseo SDK → Codex → MCP broker → Podman Worker route were demonstrated using disposable fixtures. The original host Paseo agent **did** read an external canary; an in-container same-user process **did** read a synthetic colocated credential. No experiment proved that every provider tool is forced through the broker. Therefore, full security isolation **has not passed**. Implementation may advance through F1-S1 and credential-free F1-S2 only; F1-S3 and autonomous untrusted execution remain blocked. Do not convert a limited PASS into overall SEC-01 approval.

## Questions to answer

1. Can we discover an available model, start a real Paseo session, correlate its ID with a Run, observe timeline events and recover missing history after a disconnect?
2. In which processes, user IDs, mount namespaces and environments do daemon/provider credentials and agent tool commands actually reside?
3. Can agent-controlled code access a **fake canary** beyond its workspace, a host socket, another project or a secret-like fixture? Can it write outside its allowed paths?
4. Does isolation work without lifting filesystem/network controls or co-locating untrusted code with authentic provider secrets?
5. How do the results differ for Codex and OpenCode? Do not assume their sandbox policies are equivalent.

## Experiment stages

- **A — Inventory, no agents:** capture version/output of `paseo --version`, `paseo ls`, supported `paseo provider diagnostic codex --json`/`opencode --json`, Node, Podman and selected provider binaries. Sanitize output; never dump auth files or environment secrets.
- **B — Credential-free Podman negative tests:** pin container image/version; disable network, drop capabilities, use unprivileged namespaces, read-only mounts and bounded resources. Verify that a fixture is readable but external canary, writes, forbidden egress and privileged sockets are not. This stage does **not** certify Paseo.
- **C — Paseo SDK on a disposable benign repository:** use a pinned, compatible `@getpaseo/client`; discover available models with supported provider APIs, start an agent, observe terminal state, events/timeline and reconnect/replay. Do not run untrusted code on the authenticated host.
- **D — Critical credentials boundary:** use fake secret canaries and inspect the **actual** process and mount isolation under tested provider modes. Compare host daemon/provider, containerized daemon/provider, and brokered separated execution only if the provider supports it. If untrusted commands can access credentials or host privileged endpoints, that topology is FAIL/BLOCKED. Do not treat an LLM instruction not to read secrets as enforcement.
- **E — Termination/recovery:** disconnect, cancel, restart, reconcile and ensure no duplicate execution/side effects. Rehydrate history from authoritative SDK/API mechanisms, not a live stream alone.

## PASS conditions

A complete SEC-01 report binds versions, exact commands/configuration, agent/session IDs (redacted), sandbox/mount evidence, independent positive/negative checks, crash/replay results and unresolved risks to a signed/digested artifact where possible. Record separate results for SDK functionality and OS/credential isolation. Overall `PASS` requires both; otherwise `FAIL` or `BLOCKED`.

Never use `--privileged`, host network, broad `$HOME` mounts, provider-auth directory mounts into untrusted tools, privileged `docker.sock`/`podman.sock`, or arbitrary sandbox weakening to turn a failure green.

## Documentation

- https://paseo.sh/docs/sdk/quickstart
- https://paseo.sh/docs/sdk/providers
- https://paseo.sh/docs/sdk/events
- https://paseo.sh/docs/providers
- https://paseo.sh/docs/docker
- https://docs.podman.io/
