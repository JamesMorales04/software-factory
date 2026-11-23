# Licensing provenance UI (v4 read-only fragment)

**Status:** Office UI and component tests implemented. Core v4 DTO fragment supplied by Core98. Browser/axe **NOT_TESTED** until the integrated Core route is available in a candidate-bound run.

**Decision mechanism:** display-only projection. Core remains the authority for licence review, decisions and derivation.

## Distinction shown by Office

After a v4 decision, the Licence decisions table distinguishes:

- **Licensing fact** (`licensingFactPath` / `licensingFactSha256`): the Lead-selected `role=licensing` fact. Empty means a direct text selection, rendered as `direct text selection`.
- **Derived exact text** (`licenseTextPath` / `licenseTextSha256`): the derived `purpose=license-text` asset, for example `COPYING`.
- **Raw model selection** (`modelLicenseTextPath` / `modelLicenseTextSha256`): labelled `Raw model selection (audit only)`. It is never presented as the derived text.
- **Derivation** (`derivationRule`): for example `SELECTED_LICENSING_FACT_TO_UNIQUE_LICENSE_TEXT`.
- **Documentation identity** (`path`, `license`, `outputSha256`, obligations and verdict) remains separate from the above.

`provenance.licensingFacts[]`, `provenance.licenseTexts[]` and `provenance.derivationRule` are rendered in a read-only disclosure. No action, mutation or local derivation is added. Before a decision, `license-evidence-review` remains the raw model selection; Office does not display a derived `COPYING` label there.

## Files

- `office/src/coreApi.ts` — optional v4 documentation/provenance fields.
- `office/src/views/LicenseDecisions.tsx` — documentation evidence and provenance disclosure.
- `office/src/views/LicenseDecisions.test.tsx` — v4 fixture, distinct labels, provenance disclosure and pre-decision negative assertion.
- `office/src/i18n/catalog.en.ts`, `catalog.es.ts` — EN/ES labels.
- `office/src/styles/views.css` — read-only evidence layout.
- `docs/implementation/licensing-provenance-parity-fragment.json` — parity fragment for archiver combine.

## Verification

```text
npm run typecheck                 pass
npm run build                     pass
npm test -- --no-file-parallelism 55 files / 442 tests passed
npx vitest run src/views/LicenseDecisions.test.tsx 8 passed
```

**NOT_TESTED:** integrated Core v4 response in a real browser, axe, 1440/390 layouts, independent acceptance.

## Risks

- Fields are optional so historical non-v4 records render `not recorded` rather than invented provenance.
- If Core omits the derived fields after a v4 decision, Office shows the missing state and does not substitute the model selection.
- `licensingFactPath` may be empty for a direct text selection; that is rendered explicitly, not as a raw model text.
