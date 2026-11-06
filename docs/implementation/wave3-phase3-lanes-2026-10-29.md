# Wave 3 phase 3 — lane schedule and ownership (2026-10-29)

**Integration baseline:** `2907b17` (remote), branch
`campaign/office-ux-wave3-20261028`.
**Rule:** no lane edits another lane's worktree; shared files are resolved by the
Director. `main` remains untouched.

## Active lanes

| Lane | Branch | Worktree | Owner model | Scope | Reviewer |
|---|---|---|---|---|---|
| D design/Home | `factory/office-design-polish/20261029` | `sf-wave3/design-polish` | OpenCode Go / DeepSeek V4.1 Flash | #62 tokens, shell, Home composer, screenshots | Luna visual/frontend |
| I Office v2 | `factory/office-isometric-v2/20261029` | `sf-wave3/isometric-v2` | OpenCode Go / DeepSeek V4.1 Flash | #65/#70 studio composition, characters, density | Luna visual/frontend |
| S access repairs | `factory/access-repairs/20261029` | `sf-wave3/access-repairs` | OpenCode Go / DeepSeek V4.1 Flash | #36 legacy token, #37 Tailscale boundary, #38 pairing revocation/entropy | Sol design + Luna security |
| P provider acceptance | `factory/provider-acceptance/20261029` | `sf-wave3/provider-acceptance` | OpenCode Go / DeepSeek V4.1 Flash | #60 accounting investigation, gated real UAT runbook | Sol/Luna as needed |
| Director | `campaign/office-ux-wave3-20261028` | `sf-wave3/integration` | OpenCode Go / DeepSeek V4.1 Flash | integration, branch protection, scheduling, parity/evidence | Sol/Luna |

## Planned independent lanes (start only when the prerequisite is merged)

| Lane | Issues | Depends on | Notes |
|---|---|---|---|
| Profiles/providers finish | #61/#71 | design tokens/Home | Remove remaining budget/cap controls, searchable models, advanced-only technical limits |
| Projects/GitHub onboarding | #63 | none | authorized repo discovery (public/private), local import, new project; no manual owner/repo text when connected |
| Issues Kanban | #64 | design tokens | clear board, issue-to-campaign, advanced autonomy policy out of the default flow |
| Docs/file navigation | #66/#67 | none | clickable Markdown links/anchors, exact revision provenance, bounded pagination/virtualization beyond 512 KiB |
| Merge policy | #69 | Campaign delivery slice | project-level manual default; auto-merge only with explicit project authorization + latest candidate + gate + checks + protection |
| i18n mobile sweep | #43 | design tokens | EN/ES all remaining surfaces, 320/390/834/1440 |
| Quality/live UAT | #46/#52/#60 | #73 + access stack + owner spend decision | deterministic vs live evidence kept separate |

## Conflict boundaries

- D owns `office/src/styles/tokens.css`, `components.css`, `home.css`,
  `layout.css`, shell/Home components.
- I owns `office/src/components/office/*`, `agent-office.css`, AgentOffice view.
- S owns access/auth/session/pairing/Tailscale code and the Office access views.
- P owns provider acceptance scripts/docs only.
- Director owns `office/src/coreApi.ts`, `App.tsx`, `navigation.ts`, shared CSS
  entrypoints, migrations, `native/src/bin/core.rs`, parity JSON, roadmap and
  final integration.

No two active lanes share an owned file. If a shared file is required, the lane
reports it and the Director applies the minimal integration change.

## Remaining Core/UX fixes

- Independent reviews still list conditions: real-provider billing evidence,
  OS-kill crash injection, upstream pricing accuracy, independent UAT sign-off.
- `main` protection is prepared; not applied.
- Access stack is BLOCKED until the repairs are reviewed at exact heads.
- No `main` merge is authorized by this schedule.
