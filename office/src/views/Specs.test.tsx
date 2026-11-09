import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgenticSpecBoard, AgenticSpecDetail } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Specs } from "./Specs";

const mocks = vi.hoisted(() => ({
  getAgenticSpecBoard: vi.fn(),
  getAgenticSpec: vi.fn(),
  publishAgenticSpec: vi.fn(),
  refreshAgenticSpec: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  },
}));

const spec = {
  revision: 1,
  summary: "Bounded ICS validation",
  requirements: [
    {
      id: "REQ-001",
      statement: "Invalid calendar input is rejected.",
      assertions: [
        {
          id: "ASR-001-ACC-01",
          kind: "ACCEPTANCE" as const,
          statement: "Invalid input returns an error.",
          checkId: "CHK-001-ACC-01",
        },
        {
          id: "ASR-001-NEG-01",
          kind: "NEGATIVE" as const,
          statement: "Valid input is accepted.",
          checkId: "CHK-001-NEG-01",
        },
      ],
    },
  ],
  assumptions: [],
  risks: [],
  specSha256: "a".repeat(64),
};

const board: AgenticSpecBoard = {
  columns: ["FROZEN", "QUEUED", "RUNNING", "ACCEPTED", "BLOCKED", "CANCELLED"],
  specs: [
    {
      specId: "spec-1",
      requestId: "request-1",
      projectId: "project-1",
      objective: "Add an ICS export endpoint",
      revision: 1,
      summary: spec.summary,
      specSha256: spec.specSha256,
      graphSha256: "b".repeat(64),
      frozenAt: "2026-11-08T12:00:00Z",
      column: "FROZEN",
      campaignStatus: null,
      gateOutcome: null,
      operation: null,
      publication: null,
      issues: [],
      units: [
        {
          id: "api",
          title: "API",
          objective: "Expose the exporter",
          dependsOn: [],
          paths: ["src/api.rs"],
          acceptance: ["Invalid input returns an error"],
        },
      ],
    },
  ],
};

const detail: AgenticSpecDetail = {
  frozen: {
    record: {
      id: "spec-1",
      requestId: "request-1",
      revision: 1,
      state: "FROZEN",
      draftJson: "{}",
      frozenJson: JSON.stringify(spec),
      specSha256: spec.specSha256,
      graphJson: "{}",
      graphSha256: "b".repeat(64),
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      schemaSha256: "c".repeat(64),
      requestSha256: "d".repeat(64),
      inputTokens: 10,
      outputTokens: 5,
      totalTokens: 15,
      costMicros: 1,
      usageKnown: true,
      createdAt: "2026-11-08T11:00:00Z",
      frozenAt: "2026-11-08T12:00:00Z",
    },
    spec,
  },
  draft: null,
  graph: null,
  publication: null,
  issues: [],
  githubBound: false,
};

function renderSpecs() {
  return render(
    <I18nProvider initialLocale="en">
      <MemoryRouter initialEntries={["/specs"]}>
        <Specs />
      </MemoryRouter>
    </I18nProvider>,
  );
}

describe("Specs", () => {
  beforeEach(() => {
    mocks.getAgenticSpecBoard.mockReset().mockResolvedValue(board);
    mocks.getAgenticSpec.mockReset().mockResolvedValue(detail);
    mocks.publishAgenticSpec.mockReset().mockResolvedValue({
      publication: {
        id: "publication-1",
        specId: "spec-1",
        projectId: "project-1",
        provider: "LOCAL",
        repository: "local-board",
        status: "LOCAL",
        driftStatus: "UNKNOWN",
        lastError: null,
        createdAt: "2026-11-08T12:00:00Z",
        updatedAt: "2026-11-08T12:00:00Z",
      },
      issues: [],
      local: true,
    });
    mocks.refreshAgenticSpec.mockReset();
  });

  it("renders the frozen board and traces requirements to checks", async () => {
    renderSpecs();
    await screen.findByText("Specification board");
    await userEvent.click(
      screen.getByRole("button", { name: /Add an ICS export endpoint/ }),
    );
    expect(await screen.findByText("REQ-001")).toBeDefined();
    expect(screen.getByText("ASR-001-ACC-01")).toBeDefined();
    expect(screen.getByText("Check CHK-001-NEG-01")).toBeDefined();
    expect(screen.getByText(/Issue edits and comments/)).toBeDefined();
  });

  it("records the local board fallback without a GitHub binding", async () => {
    renderSpecs();
    await screen.findByText("Specification board");
    await userEvent.click(
      screen.getByRole("button", { name: /Add an ICS export endpoint/ }),
    );
    await screen.findByText("REQ-001");
    await userEvent.click(
      screen.getByRole("button", { name: "Use local board" }),
    );
    expect(mocks.publishAgenticSpec).toHaveBeenCalledWith(
      "request-1",
      true,
      true,
    );
    expect(
      await screen.findByText("The local board projection is recorded."),
    ).toBeDefined();
  });

  it("requires confirmation before a GitHub projection", async () => {
    mocks.getAgenticSpec.mockResolvedValue({ ...detail, githubBound: true });
    renderSpecs();
    await screen.findByText("Specification board");
    await userEvent.click(
      screen.getByRole("button", { name: /Add an ICS export endpoint/ }),
    );
    await screen.findByText("REQ-001");
    const publish = screen.getByRole("button", { name: "Publish to GitHub" });
    expect((publish as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I confirm this authorized GitHub projection/,
      }),
    );
    await userEvent.click(publish);
    expect(mocks.publishAgenticSpec).toHaveBeenCalledWith(
      "request-1",
      true,
      false,
    );
  });

  it("shows an empty state before the first frozen specification", async () => {
    mocks.getAgenticSpecBoard.mockResolvedValue({ ...board, specs: [] });
    renderSpecs();
    expect(
      await screen.findByText("No frozen specification yet"),
    ).toBeDefined();
  });
});
