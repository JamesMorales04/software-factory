# F1-S4-C-06 — Browser test-server reliability and certification readiness

**Status:** Implemented and deterministically verified; zero paid sessions spent.
Final Vlog live certification remains **HUMAN_REQUIRED** pending new Product
Owner authorization.

## 1. Exact C-05 failure

Persisted OpenCode tool parts for session `ses_ea0c56859ffemM12r7xrEkbXJO`
show both server-start commands as:

- `status: error`
- `metadata.interrupted: true`
- `error: "Tool execution aborted"`
- no exit code, no stderr, no output

Commands:

```sh
cd <workspace> && PORT=4173 nohup npm start >/tmp/factory-vlog-server-1998987096.log 2>&1 & sleep 1; cat /tmp/factory-vlog-server-1998987096.log
cd <workspace> && (PORT=4173 nohup npm start >/tmp/factory-vlog-server-1998987096.log 2>&1 &) ; sleep 2; cat /tmp/factory-vlog-server-1998987096.log; curl ...
```

Evidence after the run: port 4173 was free, the server log file did not exist,
and no app process was listening. The failure was therefore **agent-managed
background process lifecycle interruption**, not an application exit, port
conflict or model-time exhaustion. The model then ended without `open`.

## 2. Corrective design

A Project may declare a Browser Test Server contract:

- explicit argv start command;
- authorized loopback endpoint (from `browserEndpoints`);
- readiness path and bounded startup timeout.

The Worker:

1. starts the command in the exact Attempt workspace, before the Paseo session;
2. sets `PORT`, `HOST=127.0.0.1` and `FACTORY_TEST_ENDPOINT` in a minimal
   credential-free environment;
3. writes logs outside the repository under a Worker-owned registry;
4. waits for the authorized endpoint to return 2xx;
5. records PID/process group/start-time/workspace ownership and reuses only a
   verified owned healthy server; identity is mandatory and an empty start
   time is never treated as a wildcard;
6. refuses to adopt a healthy unowned process on the port;
7. reuses a healthy server only after verifying its exact workspace ownership,
   and stops the owned process group on completion, failure or cancellation,
   including descendants after the group leader exits and after the Attempt
   workspace is deleted; startup cleanup uses a direct group kill, and cleanup
   state is retained if termination cannot be verified so a later reaper can
   retry;
8. reaps stale managed servers at Worker startup after a crash, in a per-Worker
   registry so one Worker cannot kill another Worker's live server; an
   exclusive registry lock refuses a second Worker with the same logical ID.

Startup failure is reported as `browser-test-server: ...` and classified
`INFRASTRUCTURE_BLOCKED`; no Paseo session is started and no paid model attempt
is consumed.

The Worker-managed server is not browser-capability evidence. Only the
Implementer's successful Factory-managed wrapper commands count. A management
prompt tells the model the server is already running and must not be restarted.

## 3. Deterministic acceptance

- **D3:** managed server starts, serves the Attempt workspace, and stops after
  the Run. The browser check passes on the candidate code.
- **D4:** an unrelated healthy process on the authorized port is rejected as
  `INFRASTRUCTURE_BLOCKED`; no Paseo session is created.
- **D5:** an immediately failing start command is diagnosed within the bounded
  timeout; no Paseo session is created.
- **D6:** cancellation stops the managed server and leaves no registry state.
- **D1/D2:** same-session continuation and failed-command rejection remain.
- **R1–R8, C1–C18** remain green.
- Unit tests cover readiness, owned reuse, unowned-port rejection, startup
  diagnosis, process-group cleanup (including descendants after leader exit),
  identity refusal, reaping, spaced command names, registry locking and
  symlinked-registry refusal.
- Linux uses pidfd handles plus `/proc` start-time/cwd verification; signals
  are sent through the pidfd so PID reuse cannot redirect them. Non-Linux
  builds fail closed before launching a child.

## 4. Zero-paid real preflight

A real preflight using the actual Vlog application, the generated Factory
skill/wrapper and real Chromium completed with **zero paid Implementer
sessions**:

- managed server started at `http://127.0.0.1:4173/`;
- wrapper `open`, `snapshot`, `close` succeeded;
- `browserCapabilityComplete` returned true;
- unauthorized `http://example.com/` was rejected;
- server and workspace were stopped/cleaned.

This proves the environment path, not model-mediated behavior.

## 5. Certification readiness

- Same-session browser completion is ready and deterministic-verified.
- The managed server removes agent-managed background startup from the critical
  path.
- The Vlog Project must be registered with the declarative `npm start` contract
  for the next live attempt.
- No paid C-06 session was spent; no new authorization is assumed.
- Proposed next run remains one bounded Vlog Implementer session with
  `maxAttempts=1`, `maxRepairs=0`.
