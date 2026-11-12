# Codex ChatGPT authentication and native review

**Status:** implemented for deterministic and protocol testing. Real inference
remains blocked until a separate bounded execution authorization is issued.

Factory uses the official Codex App Server for authentication and review. It
does not implement a custom OAuth/token store, does not copy the host
`~/.codex`, and does not expose the credential to Workers, project workspaces,
tools, prompts, browser storage, logs or CI artifacts.

## Credential placement

| Item | Location | Access |
|---|---|---|
| Codex home | named volume `factory-codex-home` mounted at `/codex-home` | `reviewer`, `codex-auth`, `codex-status` and the one-shot `init` only |
| Codex binary | `/usr/local/bin/codex` in the reviewer image | reviewer image only |
| Factory secrets | existing `/config` volumes | unchanged |

`/codex-home` is an absolute directory owned by UID/GID `10001` and mode `0700`.
The reviewer container is read-only apart from `/tmp`, `/state` and the Codex
volume. There is no host `CODEX_HOME`, host HOME, Podman socket or arbitrary
host path mount.

The reviewer writes an official Codex permissions profile named
`factory-review`. It extends `:read-only` and denies tool-sandbox reads of the
Codex home, `/config` (which holds the Core reviewer token) and `/projects`.
The app-server process loads auth/config before applying that profile to tool
execution. `codex sandbox -P factory-review` was used to verify that a denied
file/directory is unreadable while ordinary system files remain readable; see
[the isolation probe](../evidence/codex-review-isolation-2026-11-05.md).
The review features disable the shell, unified-exec, web-search and
collaboration tools. Trusted review policy and the exact SHA binding are in
developer instructions. The exact bounded diff is delivered as the user
message of an official `review/start` custom target, explicitly marked as
untrusted candidate source content. This is the only official lower-trust
content channel; the reviewer independently verifies the snapshot HEAD/tree,
generates the bounded diff from the exact SHA pair, and binds the receipt to
that candidate. Candidate text never enters the developer/system policy
channel and cannot execute code.

## Login

From the repository root:

```sh
podman compose up -d
podman compose --profile codex-auth run --rm codex-auth
```

The command prints a JSON object containing `verificationUrl` and `userCode`
(or an `authUrl` for browser mode). Complete the official ChatGPT login and
leave the command running until it prints the authenticated account. It does
not print access tokens, refresh tokens or the auth file.

Device code is the default. Browser mode is optional and still requires the
operator to open the returned URL:

```sh
FACTORY_CODEX_LOGIN_MODE=browser \
  podman compose --profile codex-auth run --rm codex-auth
```

API-key login is rejected for this review profile. Do not set
`OPENAI_API_KEY` or use `codex login --with-api-key`.

## Status

```sh
podman compose --profile codex-auth run --rm codex-status
```

The output reports the account type, the exact requested model
(`gpt-6-luna`), whether `model/list` contains it exactly once, and
`configurationReady`. `configurationReady` is true only for a ChatGPT account
with the exact Luna model. Logged out, API-key, unsupported-account,
hidden-model, duplicate-model and missing-model states are not ready, and the
command exits nonzero after printing the status JSON.

`modelAccess` remains `NOT_VERIFIED`: `model/list` is a catalog check, not
proof that the plan can use Luna. Only an actual review request can verify
access for that request; a failed request produces `NOT_TESTED`, never PASS.
Every reviewer fleet service is built from the dedicated `reviewer` image
target and mounts `factory-codex-home` at `/codex-home`; none is the verifier
image. Each role authenticates with its own token, principal and role.

## Enable native review

The installed default keeps Codex review disabled:

```yaml
# compose.yaml
FACTORY_ENABLE_CODEX_REVIEW: "0"
```

For the bounded real-provider UAT, `compose.real-provider-uat.yaml` sets it to
`1` on Core. Restart Core and the reviewer after changing it:

```sh
podman compose -f compose.yaml -f compose.real-provider-uat.yaml up -d core \
  reviewer-functional reviewer-architecture reviewer-code-quality \
  reviewer-security reviewer-testing reviewer-operability
```

Core then answers `GET /review/config` with `mechanism: "CODEX_APP_SERVER"`.
The reviewer uses `review/start` on a fresh thread targeting the exact frozen
candidate commit, with the `factory-review` permissions profile,
`approvalPolicy: never`, `allowProviderModelFallback: false`, exact model
`gpt-6-luna` and App Server provider `openai`. A different model, provider,
review thread or candidate SHA fails closed.

## Evidence and failure behavior

- A clean review must contain the canonical `No findings.` marker. Prose such
  as an overall-correctness sentence is not sufficient.
- `P0`/`P1` findings are blocking, `P2` warnings, `P3` informational.
- The review receipt uses mechanism `CODEX_APP_SERVER`, provider `codex`,
  model `gpt-6-luna`, no model reservation and a request hash binding the
  candidate, tree, thread, turn and review text.
- Missing token usage is preserved as `usageKnown: false`; a PASS receipt is
  rejected. The reviewer submits `NOT_TESTED` instead.
- Logged-out, API-key, expired/changed credential, missing Luna, wrong
  provider/model, interrupted review, failed terminal status, changed candidate
  and unreadable output all produce `NOT_TESTED` or no receipt, never PASS.
- Each review clones a disposable snapshot under `/state`, removes the remote,
  makes the tree read-only and removes it after the review. The frozen
  candidate is never modified.

## Remaining Owner actions

1. Complete the ChatGPT device-code login in the Factory-dedicated volume.
2. Enter the OpenCode Go credential through the configured provider settings.
3. Issue a separate bounded live-execution authorization before any real
   inference. No real inference is performed by this runbook.

Do not claim provider-verified subscription status from this flow. OpenCode Go
Extra Usage remains the Owner-managed assumption recorded by ADR-0031.
