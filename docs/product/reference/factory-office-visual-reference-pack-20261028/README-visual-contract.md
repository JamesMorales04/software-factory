# Factory Office — Visual reference contract v1
Date: 2026-10-28
Status: Product Owner-approved **visual direction**, not production assets or verified UI.

## Priority and reference hierarchy
1. `01-office-isometrica-referencia-principal.png` is the **primary visual anchor** for Agent Office: dark navy shell, warm cozy isometric game-management office, semirealistic 2D art, stylized human agents, wood desks, plants, glowing screens, readable overlays and dark inspector.
2. `02-layouts-office-referencia.png` is the **primary visual anchor** for the broader UI: dark navy navigation, airy light main canvases, clear blue accents, practical cards and conversational Home, consistent project and settings views.
3. `03-home-campanas-layout-referencia.png` is a **secondary compositional reference** for a conversation-first Home and campaign detail, not a mandate to display every depicted panel.

## Non-negotiable layout/style rules
- Keep a coherent design language: navy sidebar/header, well-spaced light content surfaces on ordinary pages, readable typography, subdued borders, restrained blue/turquoise highlights.
- Home is primarily a conversation, not a metric-filled operations dashboard. Avoid excessive simultaneous cards.
- Agent Office occupies a large dedicated isometric scene; keep overlays compact and an optional contextual inspector, not a full-width log/tile collection.
- The scene MUST be a dynamically composed entity scene, never a flattened reference image. Render exactly the distinct live observable agents in scope; e.g. five implementers = five distinct avatars.
- Prefab scene tiles/sprites: floor, wall, desk, chair, plant, monitor, status markers, human avatar. Dynamic placement; camera pan/zoom and filters by project/campaign.
- Preserve responsive behavior and accessible list view. Animations reflect observed Core events only; no fabricated agent thought, progress, role state, cost, or runtime activity.
- Product settings are progressive-disclosure, not cluttered with arbitrary internal caps.
- Existing security, event provenance, authorized delivery and Core state remain authoritative.

## Critical visual quality gate
- For each relevant PR, provide real Chromium screenshots in desktop and mobile viewport sizes, comparing the actual implementation against the corresponding reference.
- Evaluate density, visual hierarchy, sidebar, typography, spacing, canvas/inspector proportions, character style, scene lighting, avatars, and functional selection.
- Show 0/1/5/12/30 distinct participants with deterministic renderer fixtures in TEST ONLY; production data must come from actual Core.
- Product Owner approves screenshots/interactive demo before wide rollout.

## Not literal specifications
These generated concept images may contain small text inaccuracies, imaginary labels, invented data, impossible progress bars, unreal behavior or fixed illustrated scene geometry. They are visual direction ONLY; do not copy imaginary claims or values.
The team should establish reusable design tokens and component rules within existing React 19 UI, not simply paste these images as page backgrounds.
