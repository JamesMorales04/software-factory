import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Action, CampaignDetail as CampaignDetailData } from "../coreApi";
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
});
