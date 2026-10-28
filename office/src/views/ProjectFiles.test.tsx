import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Project,
  ProjectFileProjection,
  ProjectRefsProjection,
  ProjectTreeProjection,
} from "../coreApi";
import { CoreApiError } from "../coreApi";
import { ProjectFiles } from "./ProjectFiles";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listProjectRefs: vi.fn(),
  listProjectFiles: vi.fn(),
  getProjectFile: vi.fn(),
}));

vi.mock("../coreApi", () => {
  class MockCoreApiError extends Error {
    readonly status: number;

    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  }
  return { coreApi: mocks, CoreApiError: MockCoreApiError };
});

const baseCommit = "a".repeat(40);
const treeSha = "d".repeat(40);
const provenance = "Core read-only Git projection";

const project: Project = {
  id: "p1",
  repository: "/tmp/project",
  base_commit: baseCommit,
  created_at: "2026-10-28T10:00:00Z",
  identity: "b".repeat(64),
};

const refs: ProjectRefsProjection = {
  projectId: "p1",
  baseCommit,
  head: "main",
  refs: [{ name: "main", kind: "BRANCH", commit: baseCommit }],
  truncated: false,
  provenance,
};

const rootTree: ProjectTreeProjection = {
  projectId: "p1",
  revision: { commit: baseCommit, tree: treeSha },
  path: "",
  entries: [
    {
      name: "docs",
      path: "docs",
      kind: "DIRECTORY",
      mode: "040000",
      objectId: "c".repeat(40),
      size: null,
    },
    {
      name: "src",
      path: "src",
      kind: "DIRECTORY",
      mode: "040000",
      objectId: "e".repeat(40),
      size: null,
    },
    {
      name: "README.md",
      path: "README.md",
      kind: "FILE",
      mode: "100644",
      objectId: "f".repeat(40),
      size: 149,
    },
    {
      name: "empty.txt",
      path: "empty.txt",
      kind: "FILE",
      mode: "100644",
      objectId: "1".repeat(40),
      size: 0,
    },
    {
      name: "large.txt",
      path: "large.txt",
      kind: "FILE",
      mode: "100644",
      objectId: "2".repeat(40),
      size: 600_000,
    },
    {
      name: "logo.bin",
      path: "logo.bin",
      kind: "FILE",
      mode: "100644",
      objectId: "3".repeat(40),
      size: 12,
    },
    {
      name: "link.md",
      path: "link.md",
      kind: "SYMLINK",
      mode: "120000",
      objectId: "4".repeat(40),
      size: 9,
    },
  ],
  truncated: false,
  skippedEntries: 0,
  provenance,
};

const srcTree: ProjectTreeProjection = {
  projectId: "p1",
  revision: { commit: baseCommit, tree: treeSha },
  path: "src",
  entries: [
    {
      name: "main.ts",
      path: "src/main.ts",
      kind: "FILE",
      mode: "100644",
      objectId: "5".repeat(40),
      size: 34,
    },
  ],
  truncated: false,
  skippedEntries: 0,
  provenance,
};

const docsTree: ProjectTreeProjection = {
  projectId: "p1",
  revision: { commit: baseCommit, tree: treeSha },
  path: "docs",
  entries: [
    {
      name: "guide.md",
      path: "docs/guide.md",
      kind: "FILE",
      mode: "100644",
      objectId: "6".repeat(40),
      size: 30,
    },
  ],
  truncated: false,
  skippedEntries: 0,
  provenance,
};

const readmeContent = [
  "# Documentation",
  "",
  "Read **the docs**.",
  "",
  "- one",
  "- two",
  "",
  "```ts",
  "const x = 1;",
  "```",
  "",
  '<img src=x onerror="alert(1)">',
  "",
  "[safe](https://example.com/docs) and [unsafe](javascript:alert(1))",
].join("\n");

function fileProjection(
  path: string,
  overrides: Partial<ProjectFileProjection>,
): ProjectFileProjection {
  return {
    projectId: "p1",
    revision: { commit: baseCommit, tree: treeSha },
    path,
    name: path.split("/").pop() ?? path,
    kind: "FILE",
    mode: "100644",
    objectId: "f".repeat(40),
    size: 34,
    content: null,
    omitted: null,
    lineCount: 0,
    provenance,
    ...overrides,
  };
}

const readme = fileProjection("README.md", {
  content: readmeContent,
  lineCount: 14,
  size: readmeContent.length,
});
const main = fileProjection("src/main.ts", {
  objectId: "5".repeat(40),
  content: "const x = 1;\nconsole.log(x);\n",
  lineCount: 2,
  size: 34,
});
const guide = fileProjection("docs/guide.md", {
  objectId: "6".repeat(40),
  content: "# Guide\n\nSafe preview.\n",
  lineCount: 3,
});
const binary = fileProjection("logo.bin", {
  content: null,
  omitted: "BINARY",
  lineCount: undefined,
});
const large = fileProjection("large.txt", {
  size: 600_000,
  content: null,
  omitted: "TOO_LARGE",
  lineCount: undefined,
});
const empty = fileProjection("empty.txt", {
  size: 0,
  content: "",
  lineCount: 0,
});
const symlink = fileProjection("link.md", {
  kind: "SYMLINK",
  mode: "120000",
  size: 9,
  omitted: "NOT_A_FILE",
  lineCount: undefined,
});

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="location">{location.search}</output>;
}

function renderProjectFiles(path = "/projects/p1/files") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="/projects/:projectId/files"
          element={
            <>
              <ProjectFiles />
              <LocationProbe />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("ProjectFiles", () => {
  beforeEach(() => {
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.listProjectRefs.mockReset().mockResolvedValue(refs);
    mocks.listProjectFiles
      .mockReset()
      .mockImplementation((_projectId: string, options: { path?: string }) => {
        if (options.path === "src") return Promise.resolve(srcTree);
        if (options.path === "docs") return Promise.resolve(docsTree);
        return Promise.resolve(rootTree);
      });
    mocks.getProjectFile
      .mockReset()
      .mockImplementation((_projectId: string, options: { path: string }) => {
        switch (options.path) {
          case "README.md":
            return Promise.resolve(readme);
          case "src/main.ts":
            return Promise.resolve(main);
          case "docs/guide.md":
            return Promise.resolve(guide);
          case "logo.bin":
            return Promise.resolve(binary);
          case "large.txt":
            return Promise.resolve(large);
          case "empty.txt":
            return Promise.resolve(empty);
          case "link.md":
            return Promise.resolve(symlink);
          default:
            return Promise.reject(new CoreApiError(404, "file not found"));
        }
      });
  });

  it("renders a bounded tree with revision and repository provenance", async () => {
    renderProjectFiles();
    expect(
      await screen.findByRole("heading", { name: "Project files" }),
    ).toBeDefined();
    expect(
      await screen.findByRole("button", { name: "Open directory src" }),
    ).toBeDefined();
    expect(screen.getByText("Frozen base")).toBeDefined();
    expect(screen.getByText(/Revision aaaaaaaaaa/)).toBeDefined();
    expect(screen.getByRole("combobox", { name: "Revision" })).toBeDefined();
    expect(screen.getByText(/Showing 7 of 7 entries/)).toBeDefined();
    expect(screen.getByText(provenance)).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Open file README.md" }),
    ).toBeDefined();
  });

  it("navigates directories, filters the listing and preserves the URL", async () => {
    const user = userEvent.setup();
    renderProjectFiles();
    await user.click(
      await screen.findByRole("button", { name: "Open directory src" }),
    );
    expect(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    ).toBeDefined();
    expect(screen.getByTestId("location").textContent).toContain("path=src");
    expect(screen.getByTestId("location").textContent).toContain(
      `ref=${baseCommit}`,
    );

    const filter = screen.getByRole("searchbox", {
      name: "Filter this directory",
    });
    await user.type(filter, "missing");
    expect(await screen.findByText("No matching entries")).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    ).toBeDefined();
  });

  it("supports keyboard tree movement and opens a file", async () => {
    const user = userEvent.setup();
    renderProjectFiles();
    const docs = await screen.findByRole("button", {
      name: "Open directory docs",
    });
    docs.focus();
    await user.keyboard("{ArrowDown}");
    const src = screen.getByRole("button", { name: "Open directory src" });
    expect(document.activeElement).toBe(src);
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    ).toBeDefined();
  });

  it("shows line-numbered source and exact file metadata", async () => {
    const user = userEvent.setup();
    renderProjectFiles();
    await user.click(
      await screen.findByRole("button", { name: "Open directory src" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    );
    expect(
      await screen.findByRole("heading", { name: "main.ts" }),
    ).toBeDefined();
    const code = await screen.findByRole("region", {
      name: "src/main.ts content",
    });
    expect(within(code).getByText("1")).toBeDefined();
    expect(within(code).getByText("2")).toBeDefined();
    expect(within(code).getByText("const x = 1;")).toBeDefined();
    const viewer = screen.getByTestId("project-file-viewer");
    expect(within(viewer).getByText("100644")).toBeDefined();
    expect(within(viewer).getByText("34 B")).toBeDefined();
    expect(within(viewer).getByText("Lines")).toBeDefined();
  });

  it("renders Markdown safely and toggles to source", async () => {
    const user = userEvent.setup();
    renderProjectFiles();
    await user.click(
      await screen.findByRole("button", { name: "Open file README.md" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Documentation", level: 2 }),
    ).toBeDefined();
    expect(screen.getByText(/<img src=x/)).toBeDefined();
    expect(screen.queryByRole("link", { name: "unsafe" })).toBeNull();
    expect(
      screen.getByRole("link", { name: "safe" }).getAttribute("href"),
    ).toContain("https://example.com/docs");
    await user.click(screen.getByRole("button", { name: "Source" }));
    const code = await screen.findByRole("region", {
      name: "README.md content",
    });
    expect(within(code).getByText("```ts")).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(
      screen.getByRole("heading", { name: "Documentation", level: 2 }),
    ).toBeDefined();
  });

  it("distinguishes binary, large, empty and symlink states", async () => {
    const user = userEvent.setup();
    renderProjectFiles();
    await user.click(
      await screen.findByRole("button", { name: "Open file logo.bin" }),
    );
    expect(
      await screen.findByText(/Binary or non-UTF-8 content is not rendered/),
    ).toBeDefined();

    await user.click(
      screen.getByRole("button", { name: "Open file large.txt" }),
    );
    expect(
      await screen.findByText(/exceeds the 512 KiB Office projection limit/),
    ).toBeDefined();

    await user.click(
      screen.getByRole("button", { name: "Open file empty.txt" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Empty file" }),
    ).toBeDefined();

    await user.click(screen.getByRole("button", { name: "Open file link.md" }));
    expect(
      await screen.findByText(/Office does not follow repository symlinks/),
    ).toBeDefined();
  });

  it("surfaces a stale repository binding instead of reading unverified files", async () => {
    mocks.listProjectRefs.mockRejectedValue(
      new CoreApiError(409, "repository binding is stale"),
    );
    renderProjectFiles();
    expect(
      await screen.findByRole("heading", {
        name: "Repository binding is stale",
      }),
    ).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Back to Projects" }),
    ).toBeDefined();
  });

  it("shows an empty project state and a recoverable directory error", async () => {
    mocks.listProjects.mockResolvedValue([]);
    const first = renderProjectFiles();
    expect(
      await screen.findByRole("heading", { name: "Project not found" }),
    ).toBeDefined();
    first.unmount();

    mocks.listProjects.mockResolvedValue([project]);
    mocks.listProjectFiles.mockRejectedValue(
      new CoreApiError(503, "repository projection is unavailable"),
    );
    renderProjectFiles();
    expect(
      await screen.findByRole("heading", { name: "Directory unavailable" }),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "Retry" })).toBeDefined();
  });
});
