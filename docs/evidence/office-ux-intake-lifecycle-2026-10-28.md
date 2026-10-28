# Unified intake full lifecycle — author-run Chromium evidence (2026-10-28)

**Status:** author-run deterministic browser evidence. It is **not** independent
acceptance, not a Compose run, not live provider inference and not Product Owner
UAT. The assessment/semantic-review model gateway and the coding provider are
deterministic fixtures. Core, SQLite, Supervisor, Worker, WorkGraph scheduling,
Git candidate lineage, verifier/reviewer processes, the deterministic Quality
Gate, Vite and Chromium are real.

## Candidate identity

| Item | Value |
|---|---|
| Factory implementation candidate | `5e7f8657a3a4e6c63b37876a1ae0817a4cc9162f` |
| Branch | `campaign/office-ux-lifecycle-e2e-20261028` |
| Base | `campaign/office-ux-baseline-20261028` at `924e026` |
| Intake request | `e1157165-1e6f-4dbe-880f-837b7b446d69` |
| Campaign | `4fd4de11-7462-489f-9089-69b2a100fd6e` |
| Exact profile revision | `d5cdf397-35f9-4de7-b4da-12467cefab67` |
| Profile content hash | `b4d5a2d39a49e4042128d60561ea3e53525ffb9a2cdf90196fcf6ff9e798a729` |
| Frozen plan hash | `9e10dc489ebcc987d3159d8d002b55288fe92f2d81d46773e07a8aa6b904d48a` |
| Action | `dccd91d6-588d-4e7e-8df5-6f3672a08ee2` |
| Execution-policy delegation | `d63ecb05-10e2-4e76-a449-4f55f4ecf687` |
| Lead request | `1315fcee-cb12-42a1-b144-c4d8f2813f3f` |
| WorkGraph task / unit | `9b21d026-215d-4ff0-a2a4-3b48c43f6683` / `unit-1` |
| Run | `dc3f12f1-4906-4f09-9457-7d208ad59dc9` |
| Lead Attempt | `257b2a8d-a82d-484e-8902-a75b602d4b97` |
| Worker (claim generation 1) | `intake-full-worker` |
| Candidate | `d6a8d992-c1eb-4f4c-9e55-7ecc60122468` |
| Candidate commit | `b12a38bbb46aed13d28d295b1d3a4149a4c986ca` |
| Candidate tree | `8b779507c6c5851674af8e505f1e5c9bf4a91e46` |
| Changed paths | `src/provider.txt` |
| Verifier receipt | `549c65d0-d725-4a58-a4fa-3ffb1cc83c11` |
| Reviewer receipt | `fbca696f-916b-4e7a-9380-1307f86b95ac` |
| Reviewer mechanism | `MODEL` (`DETERMINISTIC_FIXTURE` model gateway) |
| Reviewer model reservation | `271d5dc8-fa6c-4130-9f3f-56fff4f9c69f` |
| Quality Gate | `ACCEPTED` (`independent executed evidence accepts exact candidate`) |
| Persistence | Core restart against the same SQLite database, then browser reload |

The candidate commit/tree above belong to the disposable fixture repository, not
to the Factory implementation. The Factory implementation candidate is the Git
commit in the first row.

## Exact command

```bash
CARGO_TARGET_DIR=/home/james/Escritorio/Development/sf-office-ux/target \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
FACTORY_BROWSER_NO_SANDBOX=1 \
cargo xtask e2e intake-full
```

Result (exit 0):

```json
{"ok":true,"request":"e1157165-1e6f-4dbe-880f-837b7b446d69","campaign":"4fd4de11-7462-489f-9089-69b2a100fd6e","profileRevision":"d5cdf397-35f9-4de7-b4da-12467cefab67","planHash":"9e10dc489ebcc987d3159d8d002b55288fe92f2d81d46773e07a8aa6b904d48a","mode":"DETERMINISTIC_FIXTURE","journey":"intake-full","profileContentHash":"b4d5a2d39a49e4042128d60561ea3e53525ffb9a2cdf90196fcf6ff9e798a729","actionId":"dccd91d6-588d-4e7e-8df5-6f3672a08ee2","delegationId":"d63ecb05-10e2-4e76-a449-4f55f4ecf687","leadRequestId":"1315fcee-cb12-42a1-b144-c4d8f2813f3f","taskId":"9b21d026-215d-4ff0-a2a4-3b48c43f6683","runId":"dc3f12f1-4906-4f09-9457-7d208ad59dc9","attemptId":"257b2a8d-a82d-484e-8902-a75b602d4b97","workerId":"intake-full-worker","candidateId":"d6a8d992-c1eb-4f4c-9e55-7ecc60122468","candidateCommit":"b12a38bbb46aed13d28d295b1d3a4149a4c986ca","candidateTree":"8b779507c6c5851674af8e505f1e5c9bf4a91e46","changedPaths":["src/provider.txt"],"verifierReceiptId":"549c65d0-d725-4a58-a4fa-3ffb1cc83c11","reviewerReceiptId":"fbca696f-916b-4e7a-9380-1307f86b95ac","reviewerMechanism":"MODEL","reviewerReservationId":"271d5dc8-fa6c-4130-9f3f-56fff4f9c69f","gate":"ACCEPTED","gateReasons":["independent executed evidence accepts exact candidate"],"campaignStatus":"AUTHORIZED","persistence":"CORE_RESTART_AND_BROWSER_RELOAD"}
```

## Wave 2 integration rerun

After merging the lifecycle lane into
`campaign/office-ux-wave2-integration-20261028`, the same journey passed at
implementation `bb2ce6b`:

```json
{"ok":true,"request":"51d5d5e2-affc-4d7b-b924-a772cadbae3d","campaign":"92db9d55-434a-4dc7-a97a-665c442d7b75","profileRevision":"37347a82-9ba5-4e8b-8b0d-c1454e3ee727","planHash":"c2565a5321e58cd6157519844c576c660b4cf1732148e57ff0d592a297086c91","mode":"DETERMINISTIC_FIXTURE","journey":"intake-full","candidateId":"d23f6db0-8f7e-4730-82a4-6841de1f178a","candidateCommit":"22894cae424f644a7a42bbe525be494f2623b263","gate":"ACCEPTED","persistence":"CORE_RESTART_AND_BROWSER_RELOAD"}
```

Same deterministic/fixture limits apply.

## What the journey exercised

1. Real Core with SQLite migrations, real device-session enrollment and real
   Chromium; Project, exact execution-profile revision and default route;
   explicit non-autonomous R0 execution policy.
2. Browser Home request, typed `EXECUTION_POLICY_REQUIRED` policy setup, bounded
   assessment through the deterministic model gateway, strict proposal
   validation, frozen plan hash and explicit operator approval.
3. Exact frozen identities: request, profile revision/content hash, plan hash,
   Action, delegation and Lead request. The Campaign authorization is
   `TRUSTED_LOCAL_PROVIDER` with the implementer route, `src` scope, R0 and the
   plan-derived attempt budget.
4. Worker claim/lease on the intake Campaign's Lead Attempt (`generation=1`),
   the Lead task/attempt/run lineage, and a real Git candidate based on the
   frozen base commit with the exact commit/tree identity.
5. Candidate inspection and the bounded changed-file diff projection
   (`src/provider.txt` only), all bound to the exact ActionSpec.
6. Independent verifier process receipt `PASS`/`EXECUTED` and independent
   semantic reviewer process receipt `PASS`/`EXECUTED` with `MODEL` mechanism,
   provider/model provenance, a settled model reservation and measured usage.
7. Deterministic Quality Gate `ACCEPTED` for that exact candidate; Campaign
   status remains `AUTHORIZED`, Run `ACCEPTED`, task `SUCCEEDED` with the exact
   candidate id/commit/tree.
8. Browser Campaign workspace: WorkGraph node and Gate outcome, candidate
   identity/diff, verifier and reviewer evidence, model provenance, Gate panel
   and Run/Attempt projection.
9. Browser Request workspace: `COMPLETED`, `Gate ACCEPTED`, plan hash, profile,
   Campaign `AUTHORIZED`, `1 / 1` runs/candidates.
10. Core restart against the same database and browser reload: request,
    Campaign, candidate, receipts, Gate and diff still project the same exact
    identities. No state was recreated or inferred.

## Product defect repaired

The intake detail projection previously exposed `campaignStatus` and
`gateOutcome` only on the list endpoint. The Request workspace therefore could
not render the authoritative `Gate ACCEPTED` badge after completion even though
the Gate existed. `intake_request_value` now adds both facts to every
projection. A targeted Core test
(`intake_detail_projects_campaign_status_and_latest_gate_outcome`) asserts the
detail projection after a Gate decision and that no Gate outcome is invented
before one exists.

The Vite dev-server configuration also now allows the real path of a symlinked
`node_modules`; without it a worktree whose dependency install is shared rejects
font assets with HTTP 403. This is development-server serving configuration, not
a product runtime or dependency change.

## Other commands run at the same candidate

```bash
cargo fmt --all -- --check
cargo clippy -p factory-native --bin factory-core --locked -- -D warnings
cargo test -p factory-native --bin factory-core intake
cargo xtask e2e intake
cargo xtask e2e office
cd office && npm run typecheck && npm run lint && npm test && npm run build
```

Results:

- `cargo fmt`: clean.
- Core Clippy for `factory-core`: clean with `-D warnings`.
- Core intake tests: 4 passed, including the new detail-projection test.
- `cargo xtask e2e intake`: exit 0; request `bbfc6b06-81bc-4a9d-95bb-3e58edf653f7`,
  Campaign `7b8570f2-f414-436d-be10-afb56fa30490`, plan
  `0530350bdceb24f9b30162dd7c864b911eb40009b99e8f3734aa5c726279c5cc`,
  mode `DETERMINISTIC_FAKE_MODEL`.
- `cargo xtask e2e office`: exit 0; fixture candidate
  `0d24503509244673617902e463eda5ff3c6721e2`, gate `ACCEPTED`.
- Office typecheck/lint: clean. Office tests: 23 files, 131 tests passed.
  Office production build: succeeded.

## Live-provider opt-in — NOT_TESTED / BLOCKED

`FACTORY_LIVE_PROVIDER=1` (with `FACTORY_INTAKE_FULL=1`) requires
`FACTORY_LIVE_PROVIDER_ID`, `FACTORY_LIVE_MODEL`, `FACTORY_LIVE_MODEL_URL`,
`FACTORY_LIVE_MODEL_TOKEN`, `FACTORY_LIVE_PROVIDER_BASE_URL`,
`FACTORY_LIVE_PROVIDER_CREDENTIAL` and a Worker provider sandbox
(`FACTORY_PROVIDER_SANDBOX`, plus the adapter-specific environment). When any
are absent the journey exits 2 before starting Core, a model call, a Worker or a
browser:

```json
{"ok":false,"status":"BLOCKED","mode":"LIVE_PROVIDER","reason":"FACTORY_LIVE_PROVIDER=1 requires an authorized provider route and execution environment; nothing was called.","missing":["FACTORY_LIVE_PROVIDER_ID","FACTORY_LIVE_MODEL","FACTORY_LIVE_MODEL_URL","FACTORY_LIVE_MODEL_TOKEN","FACTORY_PROVIDER_SANDBOX","FACTORY_LIVE_PROVIDER_BASE_URL","FACTORY_LIVE_PROVIDER_CREDENTIAL"]}
```

No live provider credential or paid authorization envelope was available for
this lane. **No live inference is claimed.**

## Explicit limitations

- **Deterministic fixtures.** The assessment and semantic-review gateway return
  test data, and the coding provider is the deterministic fixture transport
  inside bwrap. This is lifecycle regression evidence, not model quality.
- **Not independent.** The author ran the journey. No independent verifier,
  reviewer, Product Owner UAT or Quality Gate independent acceptance applies to
  the Factory implementation candidate.
- **No Compose.** Host Rust binaries, Vite and host Chromium were used. This does
  not prove the container-native Compose path, nested Podman engine, broker or
  installed-product journey.
- **No delivery/PR.** No GitHub branch, PR, check, merge or deployment was
  exercised. The candidate exists only in a disposable local repository.
- **Single unit.** The journey exercises one implementation WorkUnit, not
  parallel WorkGraph integration/fan-in or repair cycles.
- **Role routing.** The fixture uses the single default model gateway for the
  Lead and semantic reviewer. Per-role route selection remains lane R work.

## What this evidence supports

It supports a truthful claim that the ordinary Office intake path now runs from
Project request through policy, assessment, frozen plan, explicit operator
approval, real Core/Supervisor/Worker execution, exact candidate, independent
verifier, semantic reviewer with recorded model provenance, deterministic
Quality Gate `ACCEPTED`, changed-file projection and Core-restart/browser-reload
persistence using deterministic fixtures. It does not support a claim of live
provider inference, Compose acceptance, independent verification or product
certification.
