# Codex managed provider configuration (2026-10-26)

**Status:** **PASS** for strict managed configuration preflight with the real
native Codex App Server `0.156.1` binary, egress bridge and explicit cost
rates. **No paid inference was requested or run.** This is not a Codex live
coding acceptance and not an isolation certificate.

`factory start` launched a separate Core container and host Supervisor/Worker
with a Worker-only `FACTORY_PROVIDER_ENV_FILE` containing:

- `FACTORY_PROVIDER_KIND=codex`
- the native vendor executable (not the npm/mise shim)
- `FACTORY_PROVIDER_EGRESS_BRIDGE` and `https://api.openai.com/v1`
- a placeholder `FACTORY_PROVIDER_UPSTREAM_TOKEN`
- both `FACTORY_PROVIDER_CODEX_*_COST_MICROS_PER_MILLION` rates

`factory doctor` and `factory status` both reported the Codex profile active,
Core health 200, rootless Podman, Supervisor and Worker running. The stack was
stopped without creating a Campaign or provider session. The redacted profile
and executable version are in
[config-summary.txt](codex-managed-config-2026-10-26/config-summary.txt); logs
are in the same directory.

## Native ChatGPT authentication constraint

The installed Codex is ChatGPT-authenticated (`auth.json` contains OAuth
tokens and no API key). Factory's provider egress boundary injects one pinned
bearer credential and forwards only a bounded header subset. The ChatGPT Codex
backend requires the Codex-owned OAuth access token, token refresh and account
header to remain with Codex outside the model request. Importing those tokens
would be unsupported credential extraction; passing them into the sandbox would
break the credential boundary. The native ChatGPT route is therefore
**BLOCKED**.

The only supported Codex live authentication mode is an operator API key at
the pinned `https://api.openai.com/v1` upstream, with
`FACTORY_PROVIDER_UPSTREAM_TOKEN`; live Codex inference is **NOT_TESTED**.
The earlier GPT-6 Luna test used a test-only local ChatGPT proxy and is not
product authentication.
