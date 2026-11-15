import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  type AgenticPortfolioEntry,
  type AgenticRequestDetail,
  CoreApiError,
} from "../coreApi";
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
  recordAgenticParticipants: vi.fn(),
  getAgenticPlanning: vi.fn(),
  reviewAgenticPlan: vi.fn(),
  getAgenticSpec: vi.fn(),
  authorAgenticSpec: vi.fn(),
  freezeAgenticSpec: vi.fn(),
  getAgenticKnowledge: vi.fn(),
}));

vi.mock("../coreApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../coreApi")>();
  return { ...actual, coreApi: mocks };
});

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

const preparationDetail: AgenticRequestDetail = {
  ...detail,
  request: { ...detail.request, state: "ASSESSING" },
  lead: { state: "READY", campaignId: null, failureReason: null },
  operation: detail.operation
    ? { ...detail.operation, status: "STARTING" }
    : null,
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

const readyPlanning = {
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
  reviewCurrent: false,
  reviewRevision: null,
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
  startReady: true,
};

const approvedPlanReview = {
  id: "review-2",
  requestId: "request-1",
  planRevision: 1,
  planSha256: "a".repeat(64),
  status: "OWNER_APPROVED",
  principalId: "operator",
  rationale: "Owner approved.",
  points: 0,
  reviewedAt: "2026-11-08T12:01:00Z",
};

const pendingReviewPlanning = {
  ...readyPlanning,
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
  reviewCurrent: true,
  reviewRevision: 1,
  reviewStatus: "PENDING",
  reviewApproved: false,
  ready: false,
  startReady: false,
};

const approvedReviewPlanning = {
  ...pendingReviewPlanning,
  review: approvedPlanReview,
  reviewStatus: "OWNER_APPROVED",
  reviewApproved: true,
};

/**
 * Mirrors the real body produced by Core `review_recorded_error`
 * (`native/src/bin/core.rs`): the owner decision is durable, then the
 * environment phase fails and the real blocker is returned with
 * `reviewRecorded: true`.
 */
function recordedDecisionFailureBody() {
  return {
    error: "the frozen Lead route is unavailable: provider offline",
    code: "ENVIRONMENT_LEAD_MODEL_UNAVAILABLE",
    review: approvedPlanReview,
    reviewRecorded: true,
    environmentBlocked: false,
    workflow: null,
  };
}

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
    mocks.recordAgenticParticipants.mockReset().mockResolvedValue({
      plan: {},
      compiled: {},
    });
    mocks.getAgenticPlanning.mockReset().mockResolvedValue(readyPlanning);
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
      reviewCurrent: true,
      reviewRevision: 1,
      materialsReady: true,
      ready: false,
      startReady: false,
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
      reviewCurrent: true,
      reviewRevision: 1,
      materialsReady: false,
      ready: false,
      startReady: false,
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

  it("refreshes planning readiness after compiling the plan", async () => {
    const artifactsMissing = {
      ...readyPlanning,
      missingArtifacts: ["PLAN", "WORKGRAPH"],
      materialsReady: false,
      ready: false,
      startReady: false,
    };
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(artifactsMissing)
      .mockResolvedValue(readyPlanning);
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    renderDirector();
    expect(await screen.findByText("Missing: PLAN, WORKGRAPH")).toBeDefined();

    await userEvent.click(screen.getByRole("button", { name: "Compile plan" }));

    expect(
      await screen.findByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();
    expect(screen.queryByText("Missing: PLAN, WORKGRAPH")).toBeNull();
    expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(2);
  });

  it("refreshes planning readiness after recording the participant plan", async () => {
    const participantMissing = {
      ...readyPlanning,
      participantPlan: {
        ...readyPlanning.participantPlan,
        present: false,
        ready: false,
        reviewerCount: 0,
        missingRoles: readyPlanning.participantPlan.requiredRoles,
        missingAssignments: readyPlanning.participantPlan.requiredRoles,
      },
      materialsReady: false,
      ready: false,
      startReady: false,
    };
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(participantMissing)
      .mockResolvedValue(readyPlanning);
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    renderDirector();
    expect(
      await screen.findByText("Participant plan incomplete"),
    ).toBeDefined();

    await userEvent.click(
      screen.getByRole("button", { name: "Record participant plan" }),
    );

    expect(
      await screen.findByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();
    expect(screen.queryByText("Participant plan incomplete")).toBeNull();
    expect(mocks.recordAgenticParticipants).toHaveBeenCalledWith("request-1");
    expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(2);
  });

  it("refreshes the selected planning projection from the Refresh control", async () => {
    const artifactsMissing = {
      ...readyPlanning,
      missingArtifacts: ["PLAN"],
      materialsReady: false,
      ready: false,
      startReady: false,
    };
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(artifactsMissing)
      .mockResolvedValue(readyPlanning);
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    renderDirector();
    expect(await screen.findByText("Missing: PLAN")).toBeDefined();

    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(
      await screen.findByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();
    expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(2);
    expect(mocks.getAgenticSpec).toHaveBeenCalledTimes(2);
    expect(mocks.getAgenticPlan).toHaveBeenCalledTimes(2);
  });

  it("keeps the last confirmed planning projection on a refresh error and retries", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(readyPlanning)
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(readyPlanning);
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    renderDirector();
    expect(
      await screen.findByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();

    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByText("Planning artifacts could not be loaded."),
    ).toBeDefined();
    expect(
      screen.getByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();

    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(
        screen.queryByText("Planning artifacts could not be loaded."),
      ).toBeNull(),
    );
  });

  it("waits for the owner decision and refreshes readiness after approval", async () => {
    const pendingPlanning = {
      ...readyPlanning,
      review: {
        id: "review-pending",
        requestId: "request-1",
        planRevision: 1,
        planSha256: "a".repeat(64),
        status: "PENDING",
        principalId: "core:planning",
        rationale: "Owner decision required.",
        points: 0,
        reviewedAt: "2026-11-08T12:00:00Z",
      },
      reviewStatus: "PENDING",
      reviewApproved: false,
      ready: false,
      reviewCurrent: true,
      reviewRevision: 1,
      startReady: false,
    };
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingPlanning)
      .mockResolvedValue({
        ...readyPlanning,
        reviewStatus: "OWNER_APPROVED",
      });
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: {
        ...pendingPlanning.review,
        status: "OWNER_APPROVED",
      },
      workflow: null,
    });
    renderDirector();
    expect(
      await screen.findByText(/Core is waiting for the owner plan decision/),
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
    expect(
      await screen.findByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeDefined();
    expect(
      screen.queryByText(/Core is waiting for the owner plan decision/),
    ).toBeNull();
  });

  it("keeps a PENDING review with missing materials in preparation, not owner wait", async () => {
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    mocks.getAgenticPlanning.mockResolvedValue({
      ...readyPlanning,
      missingArtifacts: ["STACK"],
      materialsReady: false,
      reviewStatus: "PENDING",
      reviewApproved: false,
      ready: false,
      reviewCurrent: true,
      reviewRevision: 1,
      startReady: false,
    });
    renderDirector();
    expect(
      await screen.findByText(
        /Core is preparing the deterministic planning steps/,
      ),
    ).toBeDefined();
    expect(
      screen.queryByText(/Core is waiting for the owner plan decision/),
    ).toBeNull();
  });

  it("surfaces a rejected plan review without claiming autopilot preparation", async () => {
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    mocks.getAgenticPlanning.mockResolvedValue({
      ...readyPlanning,
      reviewStatus: "REJECTED",
      reviewApproved: false,
      ready: false,
      reviewCurrent: true,
      reviewRevision: 1,
      startReady: false,
    });
    renderDirector();
    expect(
      await screen.findByText(/The plan review rejected or requested changes/),
    ).toBeDefined();
    expect(
      screen.queryByText(/Autopilot owns these deterministic steps/),
    ).toBeNull();
  });

  it.each(["PENDING", "REJECTED", "OWNER_APPROVED"])(
    "does not treat a stale %s review as the current decision",
    async (status) => {
      mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
      mocks.getAgenticPlanning.mockResolvedValue({
        ...readyPlanning,
        review: {
          id: "review-stale",
          requestId: "request-1",
          planRevision: 1,
          planSha256: "a".repeat(64),
          status,
          principalId: "core:planning",
          rationale: "Previous revision.",
          points: 0,
          reviewedAt: "2026-11-08T12:00:00Z",
        },
        reviewStatus: status,
        reviewCurrent: false,
        reviewRevision: 2,
        reviewApproved: false,
        ready: false,
        startReady: false,
      });
      renderDirector();

      expect(
        await screen.findByText(
          /Core is preparing the deterministic planning steps/,
        ),
      ).toBeDefined();
      expect(
        screen.queryByText(/Core is waiting for the owner plan decision/),
      ).toBeNull();
      expect(
        screen.queryByText(/The plan review rejected or requested changes/),
      ).toBeNull();
      expect(
        screen.queryByText(
          "Core reports the planning materials and plan review as ready.",
        ),
      ).toBeNull();
    },
  );

  it("does not show start-ready when a required environment is not frozen", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...preparationDetail,
      environment: {
        required: true,
        status: "CORE_VALIDATED",
        plan: null,
        planId: null,
        planSha256: null,
        planRevision: null,
        conventions: null,
        reviews: [],
        leadApproval: null,
        spec: null,
        specSha256: null,
        builder: { descriptor: null, receipt: null },
        licenseInventory: null,
        blockers: [],
      },
    });
    mocks.getAgenticPlanning.mockResolvedValue({
      ...readyPlanning,
      startReady: false,
    });
    renderDirector();

    expect(await screen.findByText(/ENVIRONMENT_PLAN_REQUIRED/)).toBeDefined();
    expect(
      screen.queryByText(
        "Core reports the planning materials and plan review as ready.",
      ),
    ).toBeNull();
    expect(
      screen.queryByText(/Core performs the start transaction/),
    ).toBeNull();
  });

  it("refreshes a START_READY request while the operation is QUEUED", async () => {
    vi.useFakeTimers();
    try {
      mocks.getAgenticRequest.mockResolvedValue({
        ...preparationDetail,
        operation: preparationDetail.operation
          ? { ...preparationDetail.operation, status: "QUEUED" }
          : null,
      });
      mocks.getAgenticPlanning.mockResolvedValue(readyPlanning);
      renderDirector();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      mocks.getAgenticPlanning.mockClear();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(5000);
      });

      expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("reports a blocked preparation as a recoverable failure, not autopilot progress", async () => {
    mocks.getAgenticRequest.mockResolvedValue({
      ...preparationDetail,
      request: { ...preparationDetail.request, state: "BLOCKED" },
      lead: {
        state: "BLOCKED",
        campaignId: null,
        failureReason: "planning blocked",
      },
      operation: preparationDetail.operation
        ? { ...preparationDetail.operation, status: "BLOCKED" }
        : null,
    });
    mocks.getAgenticPlanning.mockResolvedValue({
      ...readyPlanning,
      missingArtifacts: ["PLAN"],
      materialsReady: false,
      reviewStatus: null,
      reviewApproved: false,
      ready: false,
    });
    renderDirector();
    expect(
      await screen.findByText(/Core reported a blocked or failed preparation/),
    ).toBeDefined();
    expect(
      screen.queryByText(/Autopilot owns these deterministic steps/),
    ).toBeNull();
    expect(
      screen.queryByText(/Core is waiting for the owner plan decision/),
    ).toBeNull();
  });

  it("resumes a stopped owner wait on focus without duplicate requests", async () => {
    mocks.getAgenticRequest.mockResolvedValue(preparationDetail);
    mocks.getAgenticPlanning.mockReset().mockResolvedValue({
      ...readyPlanning,
      reviewStatus: "PENDING",
      reviewApproved: false,
      ready: false,
      reviewCurrent: true,
      reviewRevision: 1,
      startReady: false,
    });
    renderDirector();
    expect(
      await screen.findByText(/Core is waiting for the owner plan decision/),
    ).toBeDefined();
    mocks.getAgenticPlanning.mockClear();

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(1),
    );

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    expect(mocks.getAgenticPlanning).toHaveBeenCalledTimes(1);
  });

  async function openPendingPlanReview() {
    await screen.findByRole("heading", { name: "Plan review" });
    await userEvent.type(
      screen.getByLabelText("Review rationale"),
      "Owner approved.",
    );
  }

  it("acknowledges a recorded approval whose environment preparation is blocked", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: approvedPlanReview,
      reviewRecorded: true,
      environmentBlocked: true,
      workflow: null,
    });
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText(/Lead rejected the environment plan/i),
    ).toBeDefined();
    expect(screen.queryByText(/preparation is still pending/i)).toBeNull();
    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull(),
    );
  });

  it("keeps a recorded decision truthful and refreshes after a later preparation failure", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    const body = recordedDecisionFailureBody();
    mocks.reviewAgenticPlan.mockRejectedValue(
      new CoreApiError(503, body.error, body),
    );
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText(/preparation did not finish/i),
    ).toBeDefined();
    expect(
      screen.getByText(/the frozen Lead route is unavailable/i),
    ).toBeDefined();
    expect(
      screen.queryByText("The plan review could not be recorded."),
    ).toBeNull();
    await waitFor(() =>
      expect(mocks.getAgenticPlanning.mock.calls.length).toBeGreaterThan(1),
    );
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull(),
    );
  });

  it("still reports an unrecorded plan-review failure as an error", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValue(pendingReviewPlanning);
    const body = {
      error: "a plan review requires a rationale",
      code: "RATIONALE_REQUIRED",
    };
    mocks.reviewAgenticPlan.mockRejectedValue(
      new CoreApiError(400, body.error, body),
    );
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText("a plan review requires a rationale"),
    ).toBeDefined();
    expect(screen.queryByText(/owner decision was recorded/i)).toBeNull();
  });

  it("does not invite a duplicate decision when the post-decision refresh fails", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValue(pendingReviewPlanning);
    const body = recordedDecisionFailureBody();
    mocks.reviewAgenticPlan.mockRejectedValue(
      new CoreApiError(503, body.error, body),
    );
    renderDirector();
    await openPendingPlanReview();
    mocks.getAgenticPlanning.mockRejectedValueOnce(new Error("offline"));
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText(/Core recorded this decision/i),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull();

    mocks.getAgenticPlanning.mockResolvedValue(approvedReviewPlanning);
    await userEvent.click(
      screen.getByRole("button", { name: "Refresh state" }),
    );
    await waitFor(() =>
      expect(screen.queryByText(/Core recorded this decision/i)).toBeNull(),
    );
    expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull();
  });

  it("does not acknowledge a 2xx review that omits reviewRecorded", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: approvedPlanReview,
      workflow: null,
    });
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText(
        /did not confirm that the decision was recorded/i,
      ),
    ).toBeDefined();
    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
  });

  it("does not acknowledge a 2xx review that reports reviewRecorded false", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(pendingReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: { ...approvedPlanReview, status: "PENDING" },
      reviewRecorded: false,
      environmentBlocked: false,
      workflow: null,
    });
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));

    expect(
      await screen.findByText(
        /did not confirm that the decision was recorded/i,
      ),
    ).toBeDefined();
    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
    expect(screen.getByRole("button", { name: "Approve plan" })).toBeDefined();
  });

  it("submits one plan review when the button is clicked twice before re-render", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: approvedPlanReview,
      reviewRecorded: true,
      environmentBlocked: false,
      workflow: null,
    });
    renderDirector();
    await openPendingPlanReview();
    const button = screen.getByRole("button", { name: "Approve plan" });
    await act(async () => {
      button.click();
      button.click();
      await Promise.resolve();
    });
    expect(mocks.reviewAgenticPlan).toHaveBeenCalledTimes(1);
  });

  it("does not show a late plan-review notice on another selected request", async () => {
    const secondPortfolio = {
      ...portfolio[0],
      id: "request-2",
      objective: "Second request",
    };
    const secondDetail: AgenticRequestDetail = {
      ...detail,
      request: {
        ...detail.request,
        id: "request-2",
        objective: "Second request",
      },
      plan: detail.plan ? { ...detail.plan, id: "plan-2" } : null,
      operation: detail.operation
        ? {
            ...detail.operation,
            id: "operation-request-2",
            requestId: "request-2",
          }
        : null,
    };
    mocks.listAgenticPortfolio.mockResolvedValue([
      portfolio[0],
      secondPortfolio,
    ]);
    mocks.getAgenticRequest.mockImplementation((id: string) =>
      id === "request-1"
        ? Promise.resolve(detail)
        : Promise.resolve(secondDetail),
    );
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(pendingReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    let resolveReview:
      | ((value: {
          review: typeof approvedPlanReview;
          reviewRecorded: boolean;
          environmentBlocked: boolean;
          workflow: null;
        }) => void)
      | undefined;
    mocks.reviewAgenticPlan.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveReview = resolve;
        }),
    );
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Second request/ }),
    );
    expect(
      await screen.findByRole("heading", { name: "Second request" }),
    ).toBeDefined();

    await act(async () => {
      resolveReview?.({
        review: approvedPlanReview,
        reviewRecorded: true,
        environmentBlocked: false,
        workflow: null,
      });
      await Promise.resolve();
    });

    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
    expect(
      screen.queryByText(/did not confirm that the decision was recorded/i),
    ).toBeNull();
  });

  it("does not offer a plan decision when the current review is already approved", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValue(approvedReviewPlanning);
    renderDirector();
    await screen.findByRole("heading", { name: "Plan review" });
    expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull();
    expect(
      screen.queryByText(/awaiting an authenticated owner decision/i),
    ).toBeNull();
  });

  it("offers a new decision on the same rejected revision", async () => {
    const rejectedReviewPlanning = {
      ...pendingReviewPlanning,
      review: { ...approvedPlanReview, status: "REJECTED" as const },
      reviewStatus: "REJECTED",
      reviewApproved: false,
    };
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce(rejectedReviewPlanning)
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: approvedPlanReview,
      reviewRecorded: true,
      environmentBlocked: false,
      workflow: null,
    });
    renderDirector();
    expect(
      await screen.findByText(/decision on this same revision/i),
    ).toBeDefined();
    // The previous decision's rationale is shown as history, never prefilled
    // into the new decision.
    expect(
      (screen.getByLabelText("Review rationale") as HTMLTextAreaElement).value,
    ).toBe("");
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(mocks.reviewAgenticPlan).toHaveBeenCalledWith("request-1", {
      planRevision: 1,
      planSha256: "a".repeat(64),
      decision: "APPROVE",
      rationale: "Owner approved.",
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull(),
    );
  });

  it("offers a new decision on the same changes-requested revision", async () => {
    const changesPlanning = {
      ...pendingReviewPlanning,
      review: { ...approvedPlanReview, status: "CHANGES_REQUESTED" as const },
      reviewStatus: "CHANGES_REQUESTED",
      reviewApproved: false,
    };
    mocks.getAgenticPlanning.mockReset().mockResolvedValue(changesPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: { ...approvedPlanReview, status: "CHANGES_REQUESTED" as const },
      reviewRecorded: true,
      environmentBlocked: false,
      workflow: null,
    });
    renderDirector();
    expect(
      await screen.findByText(/decision on this same revision/i),
    ).toBeDefined();
    await openPendingPlanReview();
    await userEvent.click(
      screen.getByRole("button", { name: "Request changes" }),
    );
    expect(mocks.reviewAgenticPlan).toHaveBeenCalledWith("request-1", {
      planRevision: 1,
      planSha256: "a".repeat(64),
      decision: "CHANGES_REQUESTED",
      rationale: "Owner approved.",
    });
  });

  it("does not offer a decision for a historical rejected review", async () => {
    mocks.getAgenticPlanning.mockReset().mockResolvedValue({
      ...pendingReviewPlanning,
      review: { ...approvedPlanReview, status: "REJECTED" as const },
      reviewCurrent: false,
      reviewRevision: null,
      reviewStatus: null,
    });
    renderDirector();
    await screen.findByRole("heading", { name: "Plan review" });
    expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull();
    expect(screen.queryByText(/decision on this same revision/i)).toBeNull();
  });

  it("refreshes a stale plan review without retrying the old decision", async () => {
    const mutating = vi.fn();
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce({
        ...pendingReviewPlanning,
        review: { ...approvedPlanReview, status: "REJECTED" as const },
        reviewStatus: "REJECTED",
        reviewApproved: false,
      })
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockImplementation(() => {
      mutating();
      return Promise.reject(
        new CoreApiError(
          409,
          "the plan review does not bind the latest PLAN revision",
          {
            code: "PLAN_REVIEW_STALE",
            error: "the plan review does not bind the latest PLAN revision",
          },
        ),
      );
    });
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(
      await screen.findByText(/does not bind the latest PLAN revision/i),
    ).toBeDefined();
    expect(mutating).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(mocks.getAgenticPlanning.mock.calls.length).toBeGreaterThan(1),
    );
    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
  });

  it("refreshes a withdrawn request and does not acknowledge the decision", async () => {
    const cancelledDetail: AgenticRequestDetail = {
      ...detail,
      request: { ...detail.request, state: "CANCELLED" },
    };
    mocks.getAgenticRequest
      .mockReset()
      .mockResolvedValueOnce(detail)
      .mockResolvedValue(cancelledDetail);
    mocks.getAgenticPlanning.mockReset().mockResolvedValue({
      ...pendingReviewPlanning,
      review: { ...approvedPlanReview, status: "REJECTED" as const },
      reviewStatus: "REJECTED",
      reviewApproved: false,
    });
    const message =
      "the request was withdrawn; the plan review is not recorded";
    mocks.reviewAgenticPlan.mockRejectedValue(
      new CoreApiError(409, message, {
        code: "INTAKE_WITHDRAWN",
        error: message,
      }),
    );
    renderDirector();
    await openPendingPlanReview();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(await screen.findByText(message)).toBeDefined();
    expect(screen.queryByText("The plan review was recorded.")).toBeNull();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Approve plan" })).toBeNull(),
    );
  });

  it("submits one decision on a double click for a rejected revision", async () => {
    mocks.getAgenticPlanning
      .mockReset()
      .mockResolvedValueOnce({
        ...pendingReviewPlanning,
        review: { ...approvedPlanReview, status: "REJECTED" as const },
        reviewStatus: "REJECTED",
        reviewApproved: false,
      })
      .mockResolvedValue(approvedReviewPlanning);
    mocks.reviewAgenticPlan.mockResolvedValue({
      review: approvedPlanReview,
      reviewRecorded: true,
      environmentBlocked: false,
      workflow: null,
    });
    renderDirector();
    await screen.findByText(/decision on this same revision/i);
    await userEvent.type(
      screen.getByLabelText("Review rationale"),
      "Owner approved.",
    );
    const button = screen.getByRole("button", { name: "Approve plan" });
    await act(async () => {
      button.click();
      button.click();
      await Promise.resolve();
    });
    expect(mocks.reviewAgenticPlan).toHaveBeenCalledTimes(1);
  });

  it("does not let a late response overwrite a newly selected request", async () => {
    const secondPortfolio = {
      ...portfolio[0],
      id: "request-2",
      objective: "Second request",
    };
    const secondDetail: AgenticRequestDetail = {
      ...detail,
      request: {
        ...detail.request,
        id: "request-2",
        objective: "Second request",
      },
      plan: detail.plan ? { ...detail.plan, id: "plan-2" } : null,
      operation: detail.operation
        ? {
            ...detail.operation,
            id: "operation-request-2",
            requestId: "request-2",
          }
        : null,
    };
    let resolveFirst: ((value: AgenticRequestDetail) => void) | undefined;
    mocks.listAgenticPortfolio.mockResolvedValue([
      portfolio[0],
      secondPortfolio,
    ]);
    mocks.getAgenticRequest.mockImplementation((id: string) =>
      id === "request-1"
        ? new Promise<AgenticRequestDetail>((resolve) => {
            resolveFirst = resolve;
          })
        : Promise.resolve(secondDetail),
    );
    renderDirector();

    await userEvent.click(
      await screen.findByRole("button", { name: /Second request/ }),
    );
    expect(
      await screen.findByRole("heading", { name: "Second request" }),
    ).toBeDefined();

    await act(async () => {
      resolveFirst?.(detail);
      await Promise.resolve();
    });

    expect(
      screen.getByRole("heading", { name: "Second request" }),
    ).toBeDefined();
    expect(
      screen.queryByRole("heading", {
        name: "Add an ICS export endpoint",
      }),
    ).toBeNull();
  });
});
