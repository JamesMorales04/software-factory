---
name: feature-removal
description: Remove a Factory capability completely, including dead deep links, routes, APIs, types, constraints, tests, scripts, evidence claims and docs, and prove the removal with negative evidence.
---

# Skill: removing a Factory capability without remnants

Use this skill whenever a feature, page, route, API, field, setting or
constraint is retired. A deletion is complete only when no reachable product
surface, dead deep link, type, schema, test, script, evidence manifest or status
document still implies the capability exists.

## 1. Define the deletion boundary

Write down:

- the capability or constraint being removed;
- the replacement path or the explicit statement that no replacement exists;
- the exact candidate commit and every path that may contain references;
- whether historical rows must remain readable as evidence;
- whether an old URL should redirect or return a clear not-found state.

Do not delete historical evidence that is still cited. Mark it `HISTORICAL`
instead of silently rewriting it.

## 2. Inventory before deleting

Search the whole repository, not just the obvious file:

```sh
rg -n '<capability>|<route>|<field>|<status>|<label>|<constant>' \
  --glob '!target/**' --glob '!node_modules/**' --hidden
```

Cover at least:

- Office routes, navigation, command palette, links, deep links, redirects;
- Office types, API clients, components, i18n keys, CSS, tests, fixtures;
- Core handlers, route registration, projection fields, validation, events;
- DB queries, migrations, seeds, compatibility decoders and status checks;
- Worker/Supervisor/provider adapters and scripts;
- browser/E2E/UAT scripts and expected screenshots;
- docs, ADRs, capabilities, roadmap, parity matrix and evidence manifests.

Classify each hit as `remove`, `rename`, `redirect`, `historical evidence` or
`unrelated coincidence`.

## 3. Delete in dependency order

1. Remove user actions and pages.
2. Remove navigation, command palette entries, links and deep-link targets.
3. Add redirects for old URLs when bookmarks may exist.
4. Remove API calls and types.
5. Remove Core handlers/routes and authorization branches.
6. Remove active validation and enforcement.
7. Remove schema only with a migration and an evidence-retention decision.
8. Remove tests that only assert the deleted surface; replace them with
   negative tests proving it is no longer reachable.
9. Remove docs/parity/roadmap entries that claim the capability exists.
10. Remove stale screenshots/evidence only when they are no longer cited.

## 4. Required negative evidence

Add at least one assertion per surface:

- old route redirects or renders a deliberate not-found state;
- old API route returns `404`/`405` (or a documented redirect);
- old UI control is absent;
- old type/field is absent from the active payload;
- old constraint is not enforced;
- no broken in-app link remains;
- no `topic`/`field`/`route` string remains outside historical evidence.

Use `rg` with the deleted symbols and inspect every remaining hit.

## 5. Verification checklist

- [ ] `cargo fmt`, clippy, targeted tests and the full workspace test suite pass.
- [ ] Office typecheck, lint, unit tests and production build pass.
- [ ] Browser journeys no longer click the deleted navigation item.
- [ ] Deep links resolve: hash routes, path redirects, reload/back/forward.
- [ ] No dead links from Home, Campaigns, Projects, Issues, Agents, Activity or
      Settings.
- [ ] Capability registry, roadmap, parity matrix, ADR and evidence status are
      truthful.
- [ ] The final `rg` sweep is empty except documented historical evidence.
- [ ] A reviewer can state the exact replacement path or the explicit absence of
      one.

## 6. Stop conditions

Stop and report if:

- deleting a row would destroy evidence that an accepted candidate or security
  finding still depends on;
- the only replacement is a new implicit authority path;
- a protected authorization, money or isolation boundary would be removed
  without an ADR/owner decision;
- the old URL cannot redirect without colliding with an API route;
- the deletion leaves a capability in the registry or roadmap claiming success.

## 7. References

- [Early-development compatibility](../../../docs/architecture/decisions/ADR-0006-early-development-compatibility.md)
- [Office-first feature parity](../office-feature-parity/SKILL.md)
- [Office design](../factory-office-design/SKILL.md)
- [Repository hygiene](../../../docs/engineering/code-quality.md)
