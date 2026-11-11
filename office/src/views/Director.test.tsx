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
  assessIntakeRequest: vi.fn(),
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
  assessmentReady: true,
  leadPlanReady: true,
  actionSpecReady: true,
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
    mocks.assessIntakeRequest.mockReset().mockResolvedValue({});
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
      participantPlan: {
        required: true,
        present: true,
        ready: true,
        reviewerCount: 5,
        requiredReviewerCount: 5,
        reviewerRoles: [
          "FUNCTIONAL",
          "ARCHITECTURE",
          "CODE_QUALITY",
          "SECURITY",
          "TESTING",
        ],
        requiredRoles: [
          "FUNCTIONAL",
          "ARCHITECTURE",
          "CODE_QUALITY",
          "SECURITY",
          "TESTING",
        ],
        missingRoles: [],
        unexpectedRoles: [],
        missingAssignments: [],
        graphStale: false,
        problems: [],
      },
      reviewStatus: "AUTO_APPROVED",
      reviewApproved: true,
      materialsReady: true,
      ready: true,
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

  it("gates Compile plan on the Lead assessment and runs the Core action", async () => {
    const submitted: AgenticRequestDetail = {
      ...detail,
      lead: { state: "SUBMITTED", campaignId: null, failureReason: null },
      assessmentReady: false,
      leadPlanReady: false,
      actionSpecReady: false,
    };
    const assessed: AgenticRequestDetail = {
      ...detail,
      lead: { state: "READY", campaignId: null, failureReason: null },
      assessmentReady: true,
      leadPlanReady: true,
      actionSpecReady: true,
    };
    mocks.getAgenticRequest
      .mockReset()
      .mockResolvedValueOnce(submitted)
      .mockResolvedValueOnce(assessed);
    renderDirector();
    await screen.findByRole("heading", { name: "Add an ICS export endpoint" });

    const compile = screen.getByRole("button", { name: "Compile plan" });
    expect((compile as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Lead assessment is required/)).toBeDefined();

    await userEvent.click(
      screen.getByRole("button", { name: "Run assessment" }),
    );
    expect(mocks.assessIntakeRequest).toHaveBeenCalledWith("request-1");
    expect(mocks.assessAgenticRequest).not.toHaveBeenCalled();
    expect(
      await screen.findByText(/Core recorded the Lead assessment/),
    ).toBeDefined();
    expect(screen.queryByText(/Lead assessment is required/)).toBeNull();
    expect(
      (
        screen.getByRole("button", {
          name: "Compile plan",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });

  it("keeps Compile plan disabled when the executable ActionSpec is missing", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...detail,
      assessmentReady: true,
      leadPlanReady: true,
      actionSpecReady: false,
    });
    renderDirector();
    await screen.findByRole("heading", { name: "Add an ICS export endpoint" });

    const compile = screen.getByRole("button", { name: "Compile plan" });
    expect((compile as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/executable ActionSpec is missing/)).toBeDefined();
    expect(screen.queryByRole("button", { name: "Run assessment" })).toBeNull();
  });

  it("shows required reviewer roles, assigned principals and independent receipts", async () => {
    renderDirector();
    expect(await screen.findByText("Required reviewer roles")).toBeDefined();
    expect(screen.getByText("Functional")).toBeDefined();
    expect(screen.getByText("Code quality")).toBeDefined();
    expect(
      screen.getAllByText("reviewer-functional-process").length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText("PASS · EXECUTED").length).toBe(2);
    expect(
      screen.getByText("2 of 2 independent reviewer receipts recorded"),
    ).toBeDefined();
    expect(screen.queryByText(/model diversity/i)).toBeNull();
  });

  it("marks a required role that has no receipt", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...detail,
      reviewerEvidence: {
        ...detail.reviewerEvidence,
        independentReviewerCount: 1,
        receipts: detail.reviewerEvidence.receipts.filter(
          (receipt) => receipt.role === "FUNCTIONAL",
        ),
      },
    });
    renderDirector();
    await screen.findAllByText("Implement the ICS exporter");
    expect(screen.getByText("No receipt")).toBeDefined();
    expect(screen.getByText("PASS · EXECUTED")).toBeDefined();
    expect(
      screen.getByText("1 of 2 independent reviewer receipts recorded"),
    ).toBeDefined();
  });

  it("shows a changes-requested receipt and its evidence", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...detail,
      reviewerEvidence: {
        ...detail.reviewerEvidence,
        receipts: detail.reviewerEvidence.receipts.map((receipt) =>
          receipt.role === "CODE_QUALITY"
            ? { ...receipt, result: "CHANGES_REQUESTED" }
            : receipt,
        ),
      },
    });
    renderDirector();
    await screen.findAllByText("Implement the ICS exporter");
    expect(screen.getByText("CHANGES_REQUESTED · EXECUTED")).toBeDefined();
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
    expect(screen.getByRole("alert").textContent).toContain(
      "does not bind the required roles to principals",
    );
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
      participantPlan: {
        required: true,
        present: true,
        ready: true,
        reviewerCount: 5,
        requiredReviewerCount: 5,
        reviewerRoles: [
          "FUNCTIONAL",
          "ARCHITECTURE",
          "CODE_QUALITY",
          "SECURITY",
          "TESTING",
        ],
        requiredRoles: [
          "FUNCTIONAL",
          "ARCHITECTURE",
          "CODE_QUALITY",
          "SECURITY",
          "TESTING",
        ],
        missingRoles: [],
        unexpectedRoles: [],
        missingAssignments: [],
        graphStale: false,
        problems: [],
      },
      reviewStatus: "PENDING",
      reviewApproved: false,
      materialsReady: true,
      ready: false,
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
    expect(
      screen.getByText(/awaiting an authenticated owner decision/i),
    ).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Approve plan",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
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

  it("blocks plan approval until the participant plan matches the frozen reviewer plan", async () => {
    const requiredRoles = [
      "FUNCTIONAL",
      "ARCHITECTURE",
      "CODE_QUALITY",
      "SECURITY",
      "TESTING",
    ] as const;
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
      participantPlan: {
        required: true,
        present: false,
        ready: false,
        reviewerCount: 0,
        requiredReviewerCount: requiredRoles.length,
        reviewerRoles: [],
        requiredRoles: [...requiredRoles],
        missingRoles: [...requiredRoles],
        unexpectedRoles: [],
        missingAssignments: [...requiredRoles],
        graphStale: false,
        problems: ["the participant plan is not recorded"],
      },
      reviewStatus: "PENDING",
      reviewApproved: false,
      materialsReady: false,
      ready: false,
    });
    renderDirector();
    expect(
      await screen.findByText("Participant plan incomplete"),
    ).toBeDefined();
    expect(screen.getByText(/participant plan is not recorded/i)).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Approve plan",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
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
