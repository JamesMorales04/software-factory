# Isometric Agent Office V0 — renderer self-test evidence

**Level:** author self-test, deterministic browser fixtures. **Not** live agent
activity, **not** independent acceptance and **not** Compose acceptance.

- Code candidate: `6bb5cde8f3b3ba386180976532aff963ceec8666`
- Evidence candidate (docs): `bba85f36ae619eb5d11eb86bd8fd8ac283992d7a`
- Browser: Google Chrome for Testing 156.0.8078.4 via pinned
  `playwright-core` 1.64.1, headless, isolated temporary profile.
- App: the real Office Vite bundle at `/#/agents`; Core API responses were
  intercepted with deterministic `ParticipantRecord` fixtures (12 entities,
  2 Campaigns, 1 Project). No Core, Worker, provider or Compose process ran.
- Scene density and empty-state screenshots use the developer-only
  `visual-harness.html` that was removed before commit; it rendered the same
  `AgentOfficeIsometric` component and its header states "renderer review only
  · not live activity".

## Results

| Check | Result |
|---|---|
| 12 avatars at 1440 / 834 / 390 / 320 px | 12 at each width; page overflow 0 px |
| Console errors / failed requests | 0 / 0 |
| Keyboard selection | focus first avatar + Enter → `#/agents/lead:participant-00`, inspector opens |
| Role filter | `#/agents?role=IMPLEMENTER` → 3 avatars, URL preserved |
| 30 implementers | 30 avatars in Implementation; fitted scene pans at narrow widths; no page overflow |
| 0 participants | truthful empty state; zero avatars |

Screenshots: `page-1440.png`, `page-834.png`, `page-390.png`, `page-320.png`,
`page-selected-1440.png`, `page-filtered-1440.png`, `scene-30-1440.png`,
`scene-0-1440.png`.

## NOT_TESTED

Real Core projection, Compose, independent verification, Quality Gate, live
provider inference, axe/manual accessibility audit and visual regression.
