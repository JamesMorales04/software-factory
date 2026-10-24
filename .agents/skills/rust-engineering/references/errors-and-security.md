# Errors and security reference

## Error taxonomy

Model distinctions the caller must act on:

```rust
pub enum Failure {
    Validation(String),
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict(String),
    StaleLease { expected: i64, observed: i64 },
    BudgetExhausted,
    Canceled,
    Timeout { external_state_unknown: bool },
    ProtocolIncompatible(String),
    TransientProvider(String),
    Persistence(String),
    CleanupIncomplete(String),
    ExecutionFailed(String),
}
```

- Domain/library code returns typed variants.
- Binary boundaries may add `anyhow` context, but must preserve the class for
  status mapping and evidence.
- Do not use one catch-all `Internal` for input-dependent failures.
- Do not map `Timeout` to success or to a definitive external failure without
  evidence.

## Panics and ignored results

- No production `unwrap`, `expect` or `panic!` for recoverable conditions.
- `let _ =` is acceptable only for an explicitly irrelevant result. Cleanup,
  persistence, sends and terminal reports must be observed or reconciled.
- A join error is an error. Log it and decide whether the operation is
  incomplete.

## Secrets and telemetry

- Never log tokens, credentials, raw prompts, private keys, full environment
  maps or unredacted URLs with userinfo.
- Redact at the layer that creates the record; do not trust downstream
  collectors.
- Store only digests of bearer/registration secrets where verification is the
  purpose.
- Keep provider credentials outside agent workspaces and out of command lines
  visible to other processes where the platform permits.

## Paths and processes

- Canonicalize authorized roots and verify containment. Reject `..`, symlink
  escape and unexpected device/inode replacement at security boundaries.
- Prefer directory/file handles or verified ownership labels over bare path
  re-resolution when a race could substitute the target.
- Spawn with argv, never an interpolated shell string.
- A Git worktree is revision isolation, not a sandbox. Do not label it
  otherwise.

## Fail closed

When authorization, budget, isolation, provider identity or evidence integrity
is unknown:

1. refuse the effect;
2. persist the refusal/unknown state;
3. report `BLOCKED` or `NOT_TESTED` honestly;
4. do not retry on the host or with broader privilege.

Security-relevant exceptions require independent review and cannot be granted
by a skill, prompt or model.
