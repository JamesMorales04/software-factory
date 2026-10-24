# Native Rust host runtime and containerized Office verification

**Date:** 2026-10-24 UTC. **Code candidate:** `2dd475775d53f8c1c113d3f2b9e0f1e2eed6366c` (runtime/Office serving); provider route fix `5f3fb263841c3c3be5d1fd916bee36e82aec1395`. **Host:** CachyOS kernel `7.3.0-3-cachyos`, rootless Podman `6.1.4`, `tun` loaded. Author-run, not independent acceptance.

## Host prerequisite

```
uname -r                          -> 7.3.0-3-cachyos
lsmod | grep '^tun'               -> tun 73728 0
podman info rootless              -> true
podman run --rm --network=pasta alpine:3.20 true -> exit 0
```

The earlier `/dev/net/tun` failure was a host kernel/module mismatch, not a Factory defect. It is resolved on this host.

## Factory lifecycle

- `factory doctor`: PASS for Core URL, Podman, rootless, image and Core health.
- `factory start` (test container `factory-host-test`, port `18766`, image `localhost/software-factory-core:dev`): container created, Core health HTTP 200.
- Port binding inspected: `127.0.0.1:18766 -> 8765`; no host-network mode.
- Authenticated `GET /projects`: 200; unauthenticated: 401.
- Office served from the image: `GET /` returned the built React app; real Chromium unlocked with the operator token and reached the Dashboard with Rust navigation (Workers present).
- `GET /core-api/projects` with the operator token: 200.
- Token env file mode: `600`; token values were not printed or written into evidence.
- Container inspection: `NetworkMode=bridge`, `ReadonlyRootfs=true`, `CapAdd=[]`, `no-new-privileges`, only the named data volume mounted at `/data`.
- Durable state: Worker `host-test-worker` registered, then `factory stop`/`factory start`; `GET /workers` returned the same Worker and timestamp after restart.
- `factory start` repeated while running: idempotent (`already running`), health 200.
- `factory stop`: stopped and removed the container while retaining the data volume.
- Test container, volume and token file were removed after verification.

## Office packaging

`deploy/Containerfile.core` now builds the React Office in a Node stage and copies it to `/app/office`; Core serves it under `FACTORY_OFFICE_DIR` and exposes the same API under `/core-api`. This is the deployed Office, not a Vite dev server.

## Limits

- No project/Attempt execution was run against this deployed container; project registration needs a controlled repository mount/workspace strategy that is not part of this test.
- No sandbox/isolation certification. Rootless Podman is a candidate runtime, not certified isolation.
- Image is tag-pinned, not digest-pinned.
- Windows/Podman Machine remains `NOT_TESTED`.
- The Office static shell is public; API calls require the operator token.
- The OpenCode Go route is corrected, but the previous OpenCode live attempt remains `LIVE_PROVIDER_AUTH_BLOCKED`; a new paid OpenCode turn requires fresh authorization.
