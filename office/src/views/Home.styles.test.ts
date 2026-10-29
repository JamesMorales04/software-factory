import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const home = readFileSync("src/styles/home.css", "utf8");
const layout = readFileSync("src/styles/layout.css", "utf8");
const tokens = readFileSync("src/styles/tokens.css", "utf8");

describe("Home visual-polish responsive contract", () => {
  it("stacks the composer and lightweight side surfaces before they clip", () => {
    expect(home).toContain("@media (max-width: 1180px)");
    const stacked = home.slice(home.indexOf("@media (max-width: 1180px)"));
    expect(stacked).toMatch(
      /\.home-layout\s*{\s*grid-template-columns:\s*minmax\(0,\s*1fr\);/,
    );
    expect(home).toContain("@media (max-width: 720px)");
    expect(home).toContain("@media (max-width: 540px)");
  });

  it("keeps grids and long Core text shrinkable", () => {
    expect(home).toContain("minmax(0, 1fr)");
    expect(home).toMatch(
      /\.home-request-copy a\s*{[^}]*overflow-wrap:\s*anywhere;/s,
    );
    expect(home).toMatch(
      /\.home-decision-copy a\s*{[^}]*overflow-wrap:\s*anywhere;/s,
    );
    expect(home).toMatch(
      /\.home-authorization\s*{[^}]*grid-template-columns:\s*minmax\(210px,\s*290px\)\s*minmax\(0,\s*1fr\);/s,
    );
  });

  it("gives the shell its own semantic tokens and a narrow drawer", () => {
    expect(tokens).toContain("--shell-bg:");
    expect(tokens).toContain("--shell-text:");
    expect(tokens).toContain("--shell-active-bg:");
    expect(tokens).toContain("--focus-inverse:");
    expect(layout).toContain("@media (max-width: 1000px)");
    expect(layout).toMatch(/\.workspace\s*{[^}]*min-width:\s*0;/s);
    expect(layout).toMatch(
      /\.workspace-main\s*{[^}]*max-width:\s*var\(--content-max\);/s,
    );
  });

  it("preserves reduced-motion and focus visibility", () => {
    expect(home).toMatch(/\.home-profile-details summary:focus-visible/);
    expect(home).toMatch(/\.home-status-links a:hover/);
    expect(layout).toMatch(/\.nav-link:focus-visible/);
    expect(layout).toMatch(/\.topbar-actions\s*{[^}]*display:\s*flex;/s);
  });
});
