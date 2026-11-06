import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { isMarkdownPath, Markdown } from "./Markdown";

describe("safe Markdown subset", () => {
  it("renders supported blocks without interpreting raw HTML", () => {
    const { container } = render(
      <Markdown
        source={[
          "# Documentation",
          "",
          "Read **the docs** and `source`.",
          "",
          "- one",
          "- two",
          "",
          "1. first",
          "2. second",
          "",
          "| File | State |",
          "| --- | --- |",
          "| README | tracked |",
          "",
          "```ts",
          "const x = 1;",
          "```",
          "",
          '<img src=x onerror="alert(1)">',
          "",
          "[safe](https://example.com/docs) and [unsafe](javascript:alert(1))",
        ].join("\n")}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Documentation", level: 2 }),
    ).toBeDefined();
    expect(screen.getByText("the docs").tagName).toBe("STRONG");
    expect(screen.getByText("source").tagName).toBe("CODE");
    expect(screen.getAllByRole("listitem")).toHaveLength(4);
    expect(screen.getByRole("table")).toBeDefined();
    expect(screen.getByRole("columnheader", { name: "File" })).toBeDefined();
    expect(screen.getByRole("cell", { name: "README" })).toBeDefined();
    expect(screen.getByText("const x = 1;").tagName).toBe("CODE");
    expect(screen.getByText(/<img src=x/)).toBeDefined();
    expect(screen.queryByRole("link", { name: "unsafe" })).toBeNull();
    expect(
      screen.getByRole("link", { name: "safe" }).getAttribute("href"),
    ).toContain("https://example.com/docs");
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector('a[href^="javascript:"]')).toBeNull();
  });

  it("renders blockquotes, rules and an ordered-list start", () => {
    const { container } = render(
      <Markdown
        source={["> quoted", "", "---", "", "5. five", "6. six"].join("\n")}
      />,
    );
    expect(screen.getByText("quoted").tagName).toBe("P");
    expect(screen.getByRole("blockquote")).toBeDefined();
    expect(container.querySelector("hr")).not.toBeNull();
    expect(screen.getByRole("list").getAttribute("start")).toBe("5");
  });

  it("recognizes Markdown documentation paths", () => {
    expect(isMarkdownPath("README.md")).toBe(true);
    expect(isMarkdownPath("docs/guide.MARKDOWN")).toBe(true);
    expect(isMarkdownPath("src/main.rs")).toBe(false);
  });
});
