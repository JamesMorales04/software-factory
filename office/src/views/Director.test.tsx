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
  getAgenticScheduler: vi.fn(),
  setAgenticPriority: vi.fn(),
  getAgenticPlan: vi.fn(),
  planAgenticRequest: vi.fn(),
  getAgenticPlanning: vi.fn(),
  reviewAgenticPlan: vi.fn(),
  getAgenticSpec: vi.fn(),
  authorAgenticSpec: vi.fn(),
  freezeAgenticSpec: vi.fn(),
  getAgenticKnowledge: vi.fn(),
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
    directorId: "director:owner",
    operationManagerId: "operation-manager:request-1",
    priority: 100,
    operationStatus: "EXECUTING",
    worktreePolicy: "PER_ATTEMPT",
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
  planningLane: "FORMAL_SPEC",
  planning: {
    artifacts: [],
    review: null,
    workflow: null,
  },
  findings: [],
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
    unitAssignments: [
      {
        unitId: "unit-1",
        specialization: "rust",
        paths: ["src/ics"],
        provider: "opencode",
        model: "deepseek-v4.1-flash",
        providerMode: null,
      },
    ],
    reviewerCount: 2,
    reviewerRoles: ["FUNCTIONAL", "CODE_QUALITY"],
    reviewerAssignments: [
      {
        role: "FUNCTIONAL",
        principalId: "reviewer-functional-process",
        provider: "opencode",
        model: "deepseek-v4.1-flash",
      },
      {
        role: "CODE_QUALITY",
        principalId: "reviewer-code-quality-process",
        provider: "opencode",
        model: "deepseek-v4.1-flash",
      },
    ],
  },
  reviewerEvidence: {
    requiredReviewerCount: 2,
    requiredRoles: ["FUNCTIONAL", "CODE_QUALITY"],
    assignedPrincipals: [
      {
        role: "FUNCTIONAL",
        principalId: "reviewer-functional-process",
        provider: "opencode",
        model: "deepseek-v4.1-flash",
      },
      {
        role: "CODE_QUALITY",
        principalId: "reviewer-code-quality-process",
        provider: "opencode",
        model: "deepseek-v4.1-flash",
      },
    ],
    independentReviewerCount: 2,
    receipts: [
      {
        receiptId: "receipt-functional",
        principalId: "reviewer-functional-process",
        role: "FUNCTIONAL",
        result: "PASS",
        evidence: "EXECUTED",
      },
      {
        receiptId: "receipt-code-quality",
        principalId: "reviewer-code-quality-process",
        role: "CODE_QUALITY",
        result: "PASS",
        evidence: "EXECUTED",
      },
    ],
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
  operation: {
    id: "operation-request-1",
    requestId: "request-1",
    projectId: "project-1",
    directorId: "director:owner",
    operationManagerId: "operation-manager:request-1",
    priority: 100,
    status: "EXECUTING",
    worktreePolicy: "PER_ATTEMPT",
    createdAt: "2026-11-08T11:00:00Z",
    updatedAt: "2026-11-08T12:00:00Z",
  },
  context: { handoffs: [], note: "No handoffs yet." },
  semantic: null,
};

const graph = {
  units: [
    {
      id: "unit-1",
      title: "Implement the ICS exporter",
      objective: "Add the exporter",
      dependsOn: [],
      paths: ["src/ics"],
      acceptance: ["The exporter returns a calendar"],
    },
  ],
  assumptions: [],
  risks: [],
  topologicalOrder: ["unit-1"],
  graphSha256: "c".repeat(64),
};

const planRecord = {
  requestId: "request-1",
  graphJson: JSON.stringify(graph),
  graphSha256: graph.graphSha256,
  topologicalOrderJson: JSON.stringify(graph.topologicalOrder),
  plannerProvider: "opencode",
  plannerModel: "deepseek-v4.1-flash",
  schemaSha256: "e".repeat(64),
  requestSha256: "f".repeat(64),
  inputTokens: 10,
  outputTokens: 5,
  totalTokens: 15,
  costMicros: 1,
  usageKnown: true,
  createdAt: "2026-11-08T12:00:00Z",
};

const emptySpec = {
  frozen: null,
  draft: null,
  graph: planRecord,
  publication: null,
  issues: [],
  githubBound: false,
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
    mocks.getAgenticScheduler.mockReset().mockResolvedValue({
      capacity: 4,
      active: 1,
      queued: 0,
      resourcePolicy: "CPU and available memory",
      worktreePolicy: "PER_ATTEMPT",
    });
    mocks.getAgenticPlan.mockReset().mockResolvedValue({ graph: planRecord });
    mocks.getAgenticPlanning.mockReset().mockResolvedValue({
      lane: "FORMAL_SPEC",
      signals: {
        objective: "Add an ICS export endpoint",
        greenfield: false,
        large: true,
        missingDocs: false,
        missingStack: false,
        architectureUncertainty: false,
        publicContract: true,
        dataOrSchema: false,
        securityBoundary: false,
        crossModule: false,
        changedFiles: 2,
        expectedUnits: 2,
        explicitRepair: false,
      },
      artifacts: [],
      review: null,
      workflow: null,
      missingArtifacts: [],
    });
    mocks.reviewAgenticPlan.mockReset();
    mocks.planAgenticRequest.mockReset().mockResolvedValue({
      graph: planRecord,
      validated: graph,
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      usage: {
        inputTokens: 10,
        outputTokens: 5,
        totalTokens: 15,
        costMicros: 1,
        usageKnown: true,
      },
    });
    mocks.getAgenticSpec.mockReset().mockResolvedValue(emptySpec);
    mocks.authorAgenticSpec.mockReset();
    mocks.freezeAgenticSpec.mockReset();
    mocks.getAgenticKnowledge.mockReset().mockResolvedValue({
      pack: null,
      skills: [],
      adoptions: [],
      inventory: null,
    });
    mocks.setAgenticPriority.mockReset().mockResolvedValue({
      id: "operation-request-1",
      requestId: "request-1",
      projectId: "project-1",
      directorId: "director:owner",
      operationManagerId: "operation-manager:request-1",
      priority: 10,
      status: "EXECUTING",
      worktreePolicy: "PER_ATTEMPT",
      createdAt: "2026-11-08T11:00:00Z",
      updatedAt: "2026-11-08T12:00:00Z",
    });
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
    expect(screen.getAllByText("unit-1").length).toBeGreaterThan(0);
  });

  it("shows the validated Planner WorkGraph and recompiles it", async () => {
    renderDirector();
    await screen.findAllByText("Implement the ICS exporter");
    expect(screen.getAllByText("1 unit(s)").length).toBeGreaterThan(0);
    expect(screen.getByText("Graph SHA-256")).toBeDefined();
    await userEvent.click(screen.getByRole("button", { name: "Compile plan" }));
    expect(mocks.planAgenticRequest).toHaveBeenCalledWith("request-1");
    expect(
      await screen.findByText(
        "The Planner WorkGraph was validated and recorded.",
      ),
    ).toBeDefined();
  });

  it("shows required reviewer roles, assigned principals and independent receipts", async () => {
    renderDirector();
    expect(await screen.findByText("Required reviewer roles")).toBeDefined();
    expect(screen.getByText("Functional")).toBeDefined();
    expect(screen.getByText("Code quality")).toBeDefined();
    expect(screen.getByText("reviewer-functional-process")).toBeDefined();
    expect(
      screen.getByText("2 of 2 independent reviewer receipts recorded"),
    ).toBeDefined();
    expect(screen.queryByText(/model diversity/i)).toBeNull();
  });

  it("shows unassigned required roles without inventing a principal", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...detail,
      participants: {
        ...detail.participants,
        reviewerAssignments: [],
      },
      reviewerEvidence: {
        ...detail.reviewerEvidence,
        independentReviewerCount: 0,
        receipts: [],
      },
    });
    renderDirector();
    await screen.findAllByText("Implement the ICS exporter");
    expect(screen.getAllByText("Not assigned").length).toBe(2);
    expect(
      screen.getByText("0 of 2 independent reviewer receipts recorded"),
    ).toBeDefined();
  });

  it("records an owner plan review decision", async () => {
    mocks.getAgenticPlanning.mockResolvedValue({
      lane: "FORMAL_SPEC",
      signals: {
        objective: "Add an ICS export endpoint",
        greenfield: false,
        large: true,
        missingDocs: false,
        missingStack: false,
        architectureUncertainty: false,
        publicContract: true,
        dataOrSchema: false,
        securityBoundary: false,
        crossModule: false,
        changedFiles: 2,
        expectedUnits: 2,
        explicitRepair: false,
      },
      artifacts: [],
      review: {
        id: "review-1",
        requestId: "request-1",
        planRevision: 1,
        planSha256: "a".repeat(64),
        status: "PENDING",
        principalId: "core:planning",
        rationale: "Owner decision required.",
        points: 0,
        reviewedAt: "2026-11-08T12:00:00Z",
      },
      workflow: null,
      missingArtifacts: [],
    });
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: {
        id: "review-2",
        requestId: "request-1",
        planRevision: 1,
        planSha256: "a".repeat(64),
        status: "OWNER_APPROVED",
        principalId: "operator",
        rationale: "Owner approved.",
        points: 0,
        reviewedAt: "2026-11-08T12:01:00Z",
      },
      workflow: null,
    });
    renderDirector();
    expect(
      await screen.findByRole("heading", { name: "Plan review" }),
    ).toBeDefined();
    await userEvent.type(
      screen.getByLabelText("Review rationale"),
      "Owner approved.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(mocks.reviewAgenticPlan).toHaveBeenCalledWith("request-1", {
      planRevision: 1,
      planSha256: "a".repeat(64),
      decision: "APPROVE",
      rationale: "Owner approved.",
    });
  });

  it("shows both the frozen revision and a newer draft revision", async () => {
    const frozenSpec = {
      revision: 1,
      summary: "Frozen summary",
      requirements: [],
      assumptions: [],
      risks: [],
      specSha256: "a".repeat(64),
    };
    mocks.getAgenticSpec.mockResolvedValue({
      ...emptySpec,
      frozen: {
        record: {
          ...planRecord,
          id: "spec-1",
          revision: 1,
          state: "FROZEN",
          draftJson: "{}",
          frozenJson: JSON.stringify(frozenSpec),
          specSha256: frozenSpec.specSha256,
          frozenAt: "2026-11-08T12:00:00Z",
        },
        spec: frozenSpec,
      },
      draft: {
        record: {
          ...planRecord,
          id: "spec-2",
          revision: 2,
          state: "DRAFT",
          draftJson: "{}",
          frozenJson: null,
          specSha256: null,
          frozenAt: null,
        },
        preview: { ...frozenSpec, revision: 2, specSha256: "b".repeat(64) },
      },
    });
    renderDirector();
    expect(await screen.findByText("Frozen revision 1")).toBeDefined();
    expect(screen.getByText("Draft revision 2")).toBeDefined();
  });

  it("shows the operation manager, resource governor and updates priority", async () => {
    renderDirector();
    await screen.findByText("operation-manager:request-1");
    expect(
      screen.getByText(/Resource governor: 1 active, 0 queued, capacity 4/),
    ).toBeDefined();
    await userEvent.clear(
      screen.getByLabelText("Queue priority (0 highest, 1000 lowest)"),
    );
    await userEvent.type(
      screen.getByLabelText("Queue priority (0 highest, 1000 lowest)"),
      "10",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Update priority" }),
    );
    expect(mocks.setAgenticPriority).toHaveBeenCalledWith("request-1", 10);
    expect(await screen.findByText("Priority updated.")).toBeDefined();
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
