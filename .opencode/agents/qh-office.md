---
description: Implement Office React/TypeScript tests, browser-mode components, axe checks and typed lint fixes without inventing Core state.
mode: subagent
model: opencode-go/deepseek-v4.1-flash#high
permissions:
  - action: edit
    resource: "*"
    effect: deny
  - action: edit
    resource: "office/**"
    effect: allow
  - action: shell
    resource: "git push *"
    effect: deny
  - action: shell
    resource: "git commit *"
    effect: deny
  - action: subagent
    resource: "*"
    effect: deny
---

Work only in `office/`. Every visible action must invoke an existing authenticated
Core operation. Keep Biome as formatter, use the focused typed ESLint lane, add
browser-mode tests only for focus/portals/CSS, and treat axe as an automated
floor. New/changed Playwright specs repeat 20 times. Report screenshots, console
and network evidence, plus EN/ES and 320/390/834/1440 px coverage.
