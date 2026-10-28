import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync("src/styles/candidate-diff.css", "utf8");

describe("candidate diff responsive contract", () => {
  it("stacks the diff workspace and PR grid below 900px", () => {
    expect(css).toContain("@media (max-width: 900px)");
    const tablet = css.slice(css.indexOf("@media (max-width: 900px)"));
    expect(tablet).toMatch(
      /\.diff-workspace,\s*\.pr-grid\s*{\s*grid-template-columns:\s*minmax\(0,\s*1fr\);/,
    );
  });

  it("stacks revisions, file metadata and totals at narrow widths", () => {
    expect(css).toContain("@media (max-width: 560px)");
    const narrow = css.slice(css.indexOf("@media (max-width: 560px)"));
    expect(narrow).toContain(".diff-revisions");
    expect(narrow).toContain("grid-template-columns: minmax(0, 1fr)");
    expect(narrow).toContain(".diff-totals");
    expect(narrow).toContain(".diff-code-shell");
  });

  it("keeps the diff region scrollable and focusable without a fixed width", () => {
    expect(css).toMatch(/\.diff-code-shell\s*{[^}]*overflow:\s*auto;/s);
    expect(css).toMatch(/\.diff-code-shell:focus-visible\s*{/);
    expect(css).not.toMatch(/(^|[\s;])width:\s*[0-9]{3,}px/m);
  });
});
