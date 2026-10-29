import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Action,
  CampaignAuthorization,
  CampaignDetail as CampaignDetailData,
} from "../coreApi";
import { CampaignDetail } from "./CampaignDetail";

const mocks = vi.hoisted(() => ({
  getCampaignDetail: vi.fn(),
  listActions: vi.fn(),
  listProjects: vi.fn(),
  listLeadRequests: vi.fn(),
  listProviderSessions: vi.fn(),
  listDelegations: vi.fn(),
  getGate: vi.fn(),
  getCandidateReceipts: vi.fn(),
  getLeadRequest: vi.fn(),
  cancelCampaign: vi.fn(),
  getCandidateDiff: vi.fn(),
  getProjectGitHub: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const action: Action = {
  id: "a1",
  project_id: "p1",
  objective: "Modernize the Campaign workspace.",
  provider: "opencode",
  model: "deepseek-v4.1-flash",
  risk: "R0",
  status: "AUTHORIZED",
  objective_sha256: "objective",
  created_at: "2026-10-27T10:00:00Z",
  spec_sha256: "spec",
  spec: {
    schema_version: 1,
    project_id: "p1",
    repository: "/tmp/project",
    base_commit: "0123456789abcdef",
    objective: "Modernize the Campaign workspace.",
    objective_sha256: "objective",
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    allowed_paths: ["src"],
    checks: [],
  },
};

const v2Authorization: CampaignAuthorization = {
  campaignId: "c1",
  authorizationId: "authorization-1",
  riskClass: "R0",
  executionMode: "TRUSTED_LOCAL_PROVIDER",
  actor: "Wave 3 fixture owner",
  rationale: "Fixture-only V2 authorization evidence.",
  authorizedPaths: [],
  provider: "opencode-go",
  model: "deepseek-v4.1-flash",
  providerMode: "build",
  delegationId: "delegation-1",
  allowedTools: [],
  schemaVersion: 2,
  policy: {
    policyId: "policy-1",
    authorizationId: "authorization-1",
    monetaryEnvelopeId: "envelope-1",
    scopeSha256: "a".repeat(64),
    operations: {
      modelRequestTimeoutSecs: 300,
      toolCallTimeoutSecs: 600,
      maxRequestBytes: 2097152,
      maxResponseBytes: 2097152,
      maxGenerationTokens: 8192,
      maxInFlightOperations: 4,
    },
    progress: {
      diagnoseAfterStagnantCheckpoints: 2,
      escalateAfterUnsuccessfulReplans: 1,
      observationTimeoutSecs: 300,
    },
  },
  legacy: null,
  createdAt: "2026-10-29T10:00:00Z",
};

const legacyAuthorization: CampaignAuthorization = {
  ...v2Authorization,
  authorizationId: "legacy-authorization-1",
  schemaVersion: 1,
  policy: null,
  legacy: {
    maxAttempts: 2,
    maxSessions: 4,
    maxTurns: 6,
    maxInputTokens: 40000,
    maxOutputTokens: 12000,
    maxCostMicros: 500000,
    deadlineSecs: 600,
  },
  createdAt: "2026-10-27T10:00:00Z",
};

const detail: CampaignDetailData = {
  campaign: {
    id: "c1",
    action_id: "a1",
    status: "AUTHORIZED",
    created_at: "2026-10-27T10:00:00Z",
  },
  authorization: null,
  tasks: [],
  runs: [],
  attempts: [],
  candidates: [],
  events: [],
};

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="location">{location.search}</output>;
}

function renderCampaign(path = "/campaigns/c1") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="/campaigns/:campaignId"
          element={
            <>
              <CampaignDetail />
              <LocationProbe />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("CampaignDetail", () => {
  beforeEach(() => {
    mocks.getCampaignDetail.mockReset().mockResolvedValue(detail);
    mocks.listActions.mockReset().mockResolvedValue([action]);
    mocks.listProjects.mockReset().mockResolvedValue([
      {
        id: "p1",
        repository: "/tmp/project",
        base_commit: "0123456789abcdef",
        created_at: "2026-10-27T09:00:00Z",
      },
    ]);
    mocks.listLeadRequests.mockReset().mockResolvedValue([]);
    mocks.listProviderSessions.mockReset().mockResolvedValue([]);
    mocks.listDelegations.mockReset().mockResolvedValue([]);
    mocks.getGate.mockReset();
    mocks.getCandidateReceipts.mockReset();
    mocks.getLeadRequest.mockReset();
    mocks.cancelCampaign.mockReset();
    mocks.getCandidateDiff.mockReset();
    mocks.getProjectGitHub.mockReset().mockResolvedValue({
      binding: null,
      sync_state: null,
    });
  });

  it("opens the Overview tab by default and separates lifecycle facts", async () => {
    renderCampaign();
    await screen.findByRole("heading", { name: "Campaign c1" });
    expect(
      screen
        .getByRole("tab", { name: "Overview" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    expect(screen.getByTestId("workflow-projection")).toBeDefined();
    expect(screen.getByText("Authorization not recorded")).toBeDefined();
    expect(screen.getByText(/Latest Gate:/)).toBeDefined();
  });

  it("deep-links tabs and keeps the selected tab in the URL", async () => {
    renderCampaign("/campaigns/c1?tab=workgraph");
    await screen.findByRole("heading", { name: "WorkGraph (0 units)" });
    expect(
      screen
        .getByRole("tab", { name: /WorkGraph/ })
        .getAttribute("aria-selected"),
    ).toBe("true");
    await userEvent.click(screen.getByRole("tab", { name: "Budget & usage" }));
    expect(
      screen.getByRole("heading", {
        name: "Authorization, budget and observed usage",
      }),
    ).toBeDefined();
    expect(screen.getByTestId("location").textContent).toBe("?tab=budget");
  });

  it("deep-links and preserves the selected WorkGraph unit", async () => {
    mocks.getCampaignDetail.mockResolvedValue({
      ...detail,
      tasks: [
        {
          task_id: "task-a",
          campaign_id: "c1",
          request_id: "lead-1",
          unit_id: "unit-a",
          sequence: 0,
          status: "SUCCEEDED",
          base_commit: "a".repeat(40),
          depends_on: [],
          run_id: "run-1",
          attempt_id: "attempt-1",
          candidate_id: "candidate-1",
          candidate_commit: "b".repeat(40),
          candidate_tree: "c".repeat(40),
          updated_at: "2026-10-27T10:00:00Z",
        },
        {
          task_id: "task-b",
          campaign_id: "c1",
          request_id: "lead-1",
          unit_id: "unit-b",
          sequence: 1,
          status: "PENDING",
          base_commit: "a".repeat(40),
          depends_on: ["unit-a"],
          run_id: null,
          attempt_id: null,
          candidate_id: null,
          candidate_commit: null,
          candidate_tree: null,
          updated_at: "2026-10-27T10:00:00Z",
        },
      ],
    });
    renderCampaign("/campaigns/c1?tab=workgraph&unit=unit-a");
    await screen.findByTestId("workgraph-inspector");
    expect(screen.getByTestId("location").textContent).toContain("unit=unit-a");
    await userEvent.click(
      screen.getAllByRole("button", { name: "Inspect" })[1],
    );
    await waitFor(() =>
      expect(screen.getByTestId("location").textContent).toBe(
        "?tab=workgraph&unit=unit-b",
      ),
    );
    expect(screen.getByTestId("workgraph-inspector")).toBeDefined();
  });

  it("mounts the bounded file diff and real PR inspection in the Candidates tab", async () => {
    const candidate = {
      id: "candidate-1",
      attempt_id: "attempt-1",
      action_spec_sha256: "spec",
      commit_sha: "b".repeat(40),
      tree_sha: "c".repeat(40),
      implementer_id: "worker-1",
      created_at: "2026-10-27T10:00:00Z",
    };
    mocks.getCampaignDetail.mockResolvedValue({
      ...detail,
      runs: [
        {
          id: "run-1",
          campaign_id: "c1",
          status: "SUCCEEDED",
          created_at: "2026-10-27T10:00:00Z",
        },
      ],
      attempts: [
        {
          id: "attempt-1",
          run_id: "run-1",
          status: "SUCCEEDED",
          created_at: "2026-10-27T10:00:00Z",
        },
      ],
      candidates: [candidate],
    });
    mocks.getCandidateDiff.mockResolvedValue({
      candidate_id: candidate.id,
      candidate: {
        id: candidate.id,
        attempt_id: candidate.attempt_id,
        action_spec_sha256: candidate.action_spec_sha256,
        commit_sha: candidate.commit_sha,
        tree_sha: candidate.tree_sha,
        implementer_id: candidate.implementer_id,
        created_at: candidate.created_at,
        integration: false,
        lineage: [],
      },
      gate: null,
      project_id: "p1",
      campaign_id: "c1",
      commit_sha: candidate.commit_sha,
      tree_sha: candidate.tree_sha,
      base_commit: "a".repeat(40),
      base_tree_sha: "d".repeat(40),
      changed_paths: ["src/main.rs"],
      changed_paths_truncated: false,
      diff_stat: "1 file changed, 1 insertion(+)",
      files: [
        {
          path: "src/main.rs",
          status: "MODIFIED",
          oldMode: "100644",
          newMode: "100644",
          additions: 1,
          deletions: 0,
          binary: false,
          omitted: null,
          patch: "diff --git a/src/main.rs b/src/main.rs\n+fn changed() {}\n",
          patchTruncated: false,
          patchBytes: 60,
        },
      ],
      returned_files: 1,
      files_truncated: false,
      total_additions: 1,
      total_deletions: 0,
      patch_bytes: 60,
      patch_truncated: false,
      source: "Core read-only Git file diff",
    });
    renderCampaign("/campaigns/c1?tab=candidates");
    await screen.findByTestId("candidate-diff");
    expect(screen.getByTestId("candidate-pr")).toBeDefined();
    expect(screen.getByText(/not bound to a GitHub repository/)).toBeDefined();
  });

  it("renders the V2 policy operations and progress without legacy joins crashing", async () => {
    mocks.getCampaignDetail.mockResolvedValue({
      ...detail,
      authorization: v2Authorization,
    });
    renderCampaign("/campaigns/c1?tab=budget");
    await screen.findByRole("heading", {
      name: "Authorization, budget and observed usage",
    });
    expect(screen.getByText("Schema version")).toBeDefined();
    expect(screen.getByText("policy-1")).toBeDefined();
    expect(screen.getByText("envelope-1")).toBeDefined();
    expect(screen.getByText("Model request timeout")).toBeDefined();
    expect(screen.getAllByText("300 s").length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText("2,097,152 bytes").length).toBe(2);
    expect(screen.getByText("Progress supervision")).toBeDefined();
    expect(
      screen.getByText("Diagnose after stagnant checkpoints"),
    ).toBeDefined();
    expect(screen.queryByText("Historical V1 cap evidence")).toBeNull();
    expect(screen.getAllByText("none").length).toBeGreaterThanOrEqual(2);
  });

  it("renders only the legacy V1 cap evidence when the authorization is historical", async () => {
    mocks.getCampaignDetail.mockResolvedValue({
      ...detail,
      authorization: legacyAuthorization,
    });
    renderCampaign("/campaigns/c1?tab=budget");
    await screen.findByRole("heading", {
      name: "Authorization, budget and observed usage",
    });
    expect(screen.getByText("Historical V1 cap evidence")).toBeDefined();
    expect(screen.getByText("Max turns (V1)")).toBeDefined();
    expect(screen.getByText("6")).toBeDefined();
    expect(screen.queryByText("Technical operation limits (V2)")).toBeNull();
  });

  it("does not call join on missing V2 arrays or synthesize a missing policy", async () => {
    mocks.getCampaignDetail.mockResolvedValue({
      ...detail,
      authorization: {
        ...v2Authorization,
        authorizedPaths: undefined as unknown as string[],
        allowedTools: undefined as unknown as string[],
        policy: null,
      },
    });
    renderCampaign("/campaigns/c1?tab=budget");
    await screen.findByRole("heading", {
      name: "Authorization, budget and observed usage",
    });
    expect(
      screen.getByText(
        "This V2 authorization does not contain its execution policy. Core data is incomplete; Office will not invent operation limits.",
      ),
    ).toBeDefined();
    expect(screen.getAllByText("none").length).toBeGreaterThanOrEqual(2);
  });
});
