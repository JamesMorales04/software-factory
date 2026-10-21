# Contract: Worker-managed browser test server (F1-S4-C-06)

**Status:** Implemented; experimental pre-release contract. **Version:** `1`.
TRUSTED_LOCAL only; not isolation.

## Project declaration

A Project granting `browser` may declare:

```json
"browserTestServer": {
  "command": ["npm", "start"],
  "readyPath": "/",
  "startupTimeoutSecs": 30
}
```

- `command` is a bounded argv vector (no shell string).
- `readyPath` defaults to `/` and must be a safe absolute URL path.
- `startupTimeoutSecs` defaults to 30, range 5..600.
- The first authorized `browserEndpoints` URL supplies the loopback host/port.
- The contract is frozen into the ActionSpec/binding hash.

## Worker lifecycle

1. Start the command in the exact Attempt workspace with minimal environment
   plus `HOST=127.0.0.1`, `PORT` and `FACTORY_TEST_ENDPOINT`.
2. Write logs under a Worker-owned registry, never the repository.
3. Wait for 2xx from endpoint + readyPath within the timeout.
4. Own the process group; record PID/start-time/workspace.
5. Reuse only an owned, healthy server for the same workspace/endpoint.
6. Reject a healthy unowned listener (`ErrPortOccupied`); never adopt it.
7. Stop the owned group on success, failure, cancellation and shutdown,
   including descendants after the leader exits.
8. Reap stale owned servers at Worker startup under an exclusive per-Worker
   registry lock.

Startup/readiness/port-ownership failures are `INFRASTRUCTURE_BLOCKED` before
any Paseo session. They are not model failures.

## Evidence boundary

Managed-server health is infrastructure evidence. It is not Implementer
browser-capability evidence. Only successful model-mediated wrapper `open` plus
observation/interaction counts. Independent verifier browser checks remain the
product acceptance authority.

## Limitations

TRUSTED_LOCAL processes can still ignore `HOST`/`PORT` and bind elsewhere; the
contract is control-plane policy, not OS network isolation. The managed-server
lifecycle is Linux-only and fails closed on other platforms. The server must be
able to serve the current workspace (static/reload-capable); projects needing a
rebuild-on-edit dev server must include that in the authorized command.
