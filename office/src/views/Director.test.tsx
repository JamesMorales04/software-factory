import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgenticPortfolioEntry, AgenticRequestDetail } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Director } from "./Director";

const mocks = vi.hoisted(() => ({
  listAgenticPortfolio: vi.fn(),
  getAgenticRequest: vi.fn(),
  assessAgenticRequest: vi.fn(),
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

const portfolio: AgenticPortfolioEntry[] = [
  {
    id: "request-1",
    projectId: "project-1",
    objective: "Add an ICS export endpoint",
    mode: "AUTONOMOUS",
    state: "EXECUTING",
    route: "FORMAL_SPEC",
    assurance: "ELEVATED",
    requestKind: "CHANGE",
    workUnits: 2,
    participantCount: 4,
    stackLanguages: ["rust", "typescript"],
    updatedAt: "2026-11-08T12:00:00Z",
  },
];

const detail: AgenticRequestDetail = {
  request: {
    id: "request-1",
    projectId: "project-1",
    objective: "Add an ICS export endpoint",
    mode: "AUTONOMOUS",
    state: "EXECUTING",
    profile: { id: "profile-1", revision: 2, name: "Balanced" },
    createdAt: "2026-11-08T11:00:00Z",
    updatedAt: "2026-11-08T12:00:00Z",
  },
  lead: { state: "Running", campaignId: "campaign-1", failureReason: null },
  triage: {
    requestKind: "CHANGE",
    route: "FORMAL_SPEC",
    assurance: "ELEVATED",
    mechanism: "HYBRID",
    reasons: ["The request affects a public contract."],
    unknowns: ["The OAuth library version is not pinned."],
  },
  stack: {
    languages: ["rust", "typescript"],
    frameworks: ["axum", "react"],
    runtimes: ["rust", "node"],
    packageManagers: ["cargo", "npm"],
    buildTools: ["cargo", "vite"],
    data: ["sqlite", "sqlx"],
    testFrameworks: ["cargo-test", "vitest"],
    browser: ["playwright"],
    infra: ["podman-compose"],
    securitySensitive: ["oauth"],
    conventions: [],
    evidenceRefs: ["Cargo.toml", "package.json"],
    confidence: "high",
    unknowns: [],
  },
  participants: {
    route: "FORMAL_SPEC",
    assurance: "ELEVATED",
    assignments: [
      {
        role: "Planner",
        specialization: "decomposition",
        count: 1,
        readOnly: true,
        toolPack: ["repository-read", "context-read"],
      },
      {
        role: "Implementer",
        specialization: "rust",
        count: 2,
        readOnly: false,
        toolPack: ["repository-write", "command-execute"],
      },
    ],
    contextPolicy: {
      compactAtPercent: 70,
      maxConsecutiveCompactions: 2,
      handoffAtStableMilestone: true,
      requireProbeAfterHandoff: true,
    },
    reasons: ["Formal spec requires Planner and Spec Manager."],
  },
  specifications: [
    {
      id: "unit-1",
      description: "Implement the ICS exporter",
      kind: "Implementation",
      allowedPaths: ["src/ics"],
      capabilities: ["repository-edit"],
      dependsOn: [],
      status: "PLANNED",
    },
  ],
  plan: {
    id: "plan-1",
    planHash: "a".repeat(64),
    configHash: "b".repeat(64),
    createdAt: "2026-11-08T11:30:00Z",
  },
  context: { handoffs: [], note: "No handoffs yet." },
  semantic: null,
};

function renderDirector() {
  return render(
    <I18nProvider initialLocale="en">
      <MemoryRouter initialEntries={["/director"]}>
        <Director />
      </MemoryRouter>
    </I18nProvider>,
  );
}

describe("Director", () => {
  beforeEach(() => {
    mocks.listAgenticPortfolio.mockReset().mockResolvedValue(portfolio);
    mocks.getAgenticRequest.mockReset().mockResolvedValue(detail);
    mocks.assessAgenticRequest.mockReset().mockResolvedValue({});
  });

  it("renders the owner portfolio and the compiled operation", async () => {
    renderDirector();
    expect(await screen.findByText("Owner portfolio")).toBeDefined();
    expect(
      await screen.findByRole("heading", {
        name: "Add an ICS export endpoint",
      }),
    ).toBeDefined();
    expect(screen.getAllByText("FORMAL_SPEC").length).toBeGreaterThan(0);
    expect(screen.getAllByText("ELEVATED").length).toBeGreaterThan(0);
    expect(screen.getByText("Planner")).toBeDefined();
    expect(screen.getByText("Implementer")).toBeDefined();
    expect(
      screen.getByText(
        "At 70% of effective context; at most 2 consecutive compactions",
      ),
    ).toBeDefined();
    expect(screen.getByText("unit-1")).toBeDefined();
  });

  it("runs semantic triage and displays the recorded recommendation", async () => {
    const semanticDetail: AgenticRequestDetail = {
      ...detail,
      semantic: {
        provider: "opencode-go",
        model: "deepseek-v4.1-flash",
        createdAt: "2026-11-08T12:05:00Z",
        usage: {
          inputTokens: 1200,
          outputTokens: 300,
          totalTokens: 1500,
          costMicros: 42,
          usageKnown: true,
        },
        recommendation: {
          requestKind: "CHANGE",
          route: "FORMAL_SPEC",
          assurance: "ELEVATED",
          skillNeeds: ["ics-rust"],
          unknowns: ["OAuth client registration"],
        },
      },
    };
    mocks.getAgenticRequest
      .mockResolvedValueOnce(detail)
      .mockResolvedValueOnce(semanticDetail);
    renderDirector();
    await screen.findByRole("heading", { name: "Add an ICS export endpoint" });
    await userEvent.click(
      screen.getByRole("button", { name: "Run semantic triage" }),
    );
    expect(mocks.assessAgenticRequest).toHaveBeenCalledWith("request-1");
    expect(
      await screen.findByText(
        "Semantic triage recorded. Core applied its deterministic floors.",
      ),
    ).toBeDefined();
    expect(
      await screen.findByText("opencode-go/deepseek-v4.1-flash"),
    ).toBeDefined();
    expect(screen.getByText(/ics-rust/)).toBeDefined();
  });

  it("shows an empty portfolio without inventing operations", async () => {
    mocks.listAgenticPortfolio.mockResolvedValue([]);
    renderDirector();
    expect(await screen.findByText("No operations yet")).toBeDefined();
    expect(mocks.getAgenticRequest).not.toHaveBeenCalled();
  });

  it("surfaces a portfolio error with a retry action", async () => {
    mocks.listAgenticPortfolio.mockRejectedValue(new Error("offline"));
    renderDirector();
    expect(
      await screen.findByText("The Director portfolio could not be loaded."),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "Retry" })).toBeDefined();
  });
});
