# Codex review isolation probe — 2026-11-05

**Candidate:** `d55707ef21379ee37830f2049a22ab8fa44fecc4` (plus the local
uncommitted no-shell follow-up at the time of the probe).
**Provider binary:** official Codex App Server `codex-cli 0.165.0`.
**Inference:** none. These are local protocol/sandbox probes.

## Permission profile

The reviewer writes a dedicated `config.toml` with:

```toml
default_permissions = "factory-review"

[permissions.factory-review]
extends = ":read-only"

[permissions.factory-review.filesystem]
"<codex-home>" = "deny"
"/config" = "deny"
"/projects" = "deny"

[features]
shell_tool = false
unified_exec = false
web_search_request = false
web_search_cached = false
standalone_web_search = false
multi_agent = false
multi_agent_v2 = false
```

`codex app-server --strict-config --print-startup-config` accepted the profile.

## Sandbox denial

With a temporary home and a secret file/directory:

```sh
CODEX_HOME=<home> codex sandbox -P factory-review -C /tmp -- cat <home>/config.toml
# exit 1, "Permission denied"

CODEX_HOME=<home> codex sandbox -P factory-review -C /tmp -- cat /etc/hostname
# exit 0, prints the hostname
```

A denied directory containing a token was also unreadable. A review snapshot
is read-only and the tool sandbox has no network. The final reviewer image was
also probed with a read-only root and a mounted secret: `codex sandbox
-P factory-review` denied the mounted secret.

## Tool surface

`codex debug prompt-input` with the same config produced a model-visible tool
surface containing no `exec_command`/shell declaration; `shell_tool = false`.
The review subagent also disables web search and collaboration tools. The exact
frozen diff is supplied in developer instructions so review does not need
project-code execution.

## Status surface

The final Compose reviewer image, run with the dedicated volume:

```sh
podman compose --profile codex-auth run --rm codex-status
```

printed `configurationReady:false` / `modelAccess:"NOT_VERIFIED"` for a logged
out account and exited nonzero. `model/list` listed `gpt-6-luna`, but this is a
catalog check, not entitlement evidence.

## Residual

Real ChatGPT login and live Luna review remain `NOT_TESTED`. This document is
author-run probe evidence, not independent acceptance.
