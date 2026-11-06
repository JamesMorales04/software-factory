import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const home = readFileSync("src/styles/home.css", "utf8");
const layout = readFileSync("src/styles/layout.css", "utf8");
const tokens = readFileSync("src/styles/tokens.css", "utf8");

describe("Home conversation responsive contract", () => {
  it("stacks the composer and conversation rail before they clip", () => {
    expect(home).toContain("@media (max-width: 1180px)");
    const stacked = home.slice(home.indexOf("@media (max-width: 1180px)"));
    expect(stacked).toMatch(
      /\.home-layout\s*{\s*grid-template-columns:\s*minmax\(0,\s*1fr\);/,
    );
    expect(stacked).toMatch(/\.home-thread\s*{\s*grid-column:\s*1\s*\/\s*-1;/);
    expect(home).toContain("@media (max-width: 900px)");
    expect(home).toContain("@media (max-width: 720px)");
    expect(home).toContain("@media (max-width: 620px)");
    expect(home).toContain("@media (max-width: 540px)");
  });

  it("keeps the composer toolbar and long Core text shrinkable", () => {
    expect(home).toContain("minmax(0, 1fr)");
    expect(home).toMatch(
      /\.home-context-chips\s*{\s*display:\s*grid;\s*grid-template-columns:\s*minmax\(0,\s*0\.9fr\)\s*minmax\(0,\s*1\.12fr\)\s*minmax\(128px,\s*0\.62fr\);/s,
    );
    expect(home).toMatch(
      /\.home-composer-toolbar\s*{\s*display:\s*grid;\s*grid-template-columns:\s*minmax\(0,\s*1fr\)\s*auto;/s,
    );
    expect(home).toMatch(
      /\.home-request-copy a\s*{[^}]*overflow-wrap:\s*anywhere;/s,
    );
    expect(home).toMatch(
      /\.home-decision-copy a\s*{[^}]*overflow-wrap:\s*anywhere;/s,
    );
    expect(home).toMatch(
      /\.home-thread-meta\s*{[^}]*overflow-wrap:\s*anywhere;/s,
    );
    expect(home).toMatch(
      /\.home-thread-bubble p\s*{[^}]*overflow-wrap:\s*anywhere;/s,
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
    expect(home).toMatch(/\.home-suggestion:focus-visible/);
    expect(home).toMatch(/\.home-thread-continue:focus-visible/);
    expect(home).toMatch(/\.home-profile-details summary:focus-visible/);
    expect(home).toMatch(/\.home-status-links a:hover/);
    expect(layout).toMatch(/\.nav-link:focus-visible/);
    expect(layout).toMatch(/\.topbar-actions\s*{[^}]*display:\s*flex;/s);
  });
});
