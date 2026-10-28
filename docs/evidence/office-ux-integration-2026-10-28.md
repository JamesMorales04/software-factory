# Office UX integration — author-run Chromium evidence (2026-10-28)

**Status:** author-run deterministic regression evidence. It is **not**
independent acceptance, not a Compose run, not live provider inference and not
Product Owner UAT.

## Candidate identity

| Item | Value |
|---|---|
| Factory implementation commit | `260851a3c7488e5b4d289e34105b7537c2c8902e` |
| Campaign baseline | `campaign/office-ux-baseline-20261028` at `56cfcd9` |
| Integration branch | `campaign/office-ux-integration-20261028` |
| Test candidate commit (disposable fixture repo) | `18faf4411309f57168ff63014e514a8cf021e8bf` |
| Quality Gate decision | `ACCEPTED` for that candidate |
| Browser | Chromium 1248 (`/home/james/.cache/ms-playwright`) |
| Runtime | host Rust binaries + Vite dev server; fixture assessment/provider |

## Exact command

```bash
CARGO_TARGET_DIR=/home/james/Escritorio/Development/sf-office-ux/target \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
FACTORY_BROWSER_NO_SANDBOX=1 \
node office/e2e-browser/rust-journey.mjs
```

Result (exit 0):

```json
{"ok":true,"candidate":"18faf4411309f57168ff63014e514a8cf021e8bf","gate":"ACCEPTED"}
```

## What the journey exercised

- Device-session enrollment and Home landing at `/`.
- Primary/Advanced navigation, Decision Inbox, Projects, Action detail,
  Campaign list/detail, Agent Office visual/table/timeline inspectors, Runs,
  Workers, Runtime diagnostics, Settings, Lead and Access & Devices entrypoint.
- Legacy governed fixture path: Action → Campaign authorization → Run/Attempt →
  Worker candidate → verifier receipt → independent reviewer → deterministic
  Quality Gate `ACCEPTED`; Campaign workspace tabs, WorkGraph, candidate diff
  summary, evidence, gate, events, cancellation and reload persistence.
- The protected Lead boundary: a Lead-owned Action is not given a second
  Campaign; the provider-session fixture uses a separate non-Lead Action.
- Viewport overflow checks at 834 px and 390 px for Campaign, Runtime and
  Agent Office; desktop at 1440 px.
- A bounded retry for SQLite writer contention only where the mutation is
  idempotent.

## Known limitations

- **Not independent.** The author ran this journey. An independent verifier and
  reviewer have not accepted it.
- **No Compose.** Host binaries and Vite were used. The host Podman default
  overlay store is unsupported on btrfs; an isolated VFS store was verified but
  the Compose stack was not run for this candidate.
- **Fixture, not live inference.** `FACTORY_LEAD_FIXTURE_ASSESSOR=1` and the
  deterministic Worker provider were used. No paid provider/model was called.
- **Intake is not implemented.** `/core-api/intake/requests` is intentionally
  absent; Home renders its unavailable state and the journey filters only that
  known 404. The new request → assessment → campaign journey is not claimed.
- **320 px and QR phone journey** were not exercised here.
- The two host-OpenCode sandbox tests
  (`provider_sandbox::actual_opencode_executable_uses_the_egress_broker` and
  `confined_opencode_reaches_the_pinned_egress_broker_without_a_host_credential`)
  fail identically on the untouched baseline `56cfcd9` in this environment.
  They are recorded as environmental/pre-existing, not caused by this candidate.
- `cargo test --workspace` is therefore not green; all other workspace targets
  passed. The exact failing target is `factory-native --test provider_sandbox`.
- Migrations 1–28 applied successfully to a fresh database via
  `cargo xtask migrate`.

## What this evidence supports

It supports a truthful claim that the integrated Office shell, navigation,
localization, profiles, file browser, Issues view and legacy governed
fixture/reviewer/gate path load and operate together at the tested candidate. It
does not support a claim that the new unified-intake journey, live provider
coding, Compose deployment, isolation or product acceptance is complete.
