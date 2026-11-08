import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { IntakeRequestSummary, Project } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Requests } from "./Requests";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listIntakeRequests: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const project: Project = {
  id: "project-1",
  name: "Widgets",
  repository: "/projects/widgets",
  base_commit: "a".repeat(40),
  created_at: "2026-11-08T00:00:00Z",
  source_kind: "local",
};

const request: IntakeRequestSummary = {
  id: "request-1",
  projectId: project.id,
  objective: "Fix the checkout flow",
  mode: "REVIEW_PLAN",
  state: "AWAITING_APPROVAL",
  stateDetail: null,
  profile: {
    id: "profile-1",
    name: "Balanced",
    revision: 1,
    revisionId: "revision-1",
    contentHash: "b".repeat(64),
  },
  actionId: "action-1",
  delegationId: "delegation-1",
  leadRequestId: "lead-1",
  campaignId: null,
  authorization: { required: true, available: true, reason: null },
  createdAt: "2026-11-08T00:00:00Z",
  updatedAt: "2026-11-08T01:00:00Z",
};

function renderRequests() {
  return render(
    <MemoryRouter initialEntries={["/requests"]}>
      <I18nProvider initialLocale="en">
        <Requests />
      </I18nProvider>
    </MemoryRouter>,
  );
}

describe("Requests", () => {
  beforeEach(() => {
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.listIntakeRequests.mockReset().mockResolvedValue([request]);
  });

  it("renders objective, Project name and human state instead of raw IDs", async () => {
    renderRequests();
    expect(await screen.findByText("Fix the checkout flow")).toBeDefined();
    expect(screen.getAllByText(/Widgets/).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Awaiting approval").length).toBeGreaterThan(0);
    expect(screen.queryByText("request-1")).toBeNull();
  });

  it("shows an empty state with a next action", async () => {
    mocks.listIntakeRequests.mockResolvedValue([]);
    renderRequests();
    expect(await screen.findByText("No requests match")).toBeDefined();
    expect(screen.getByRole("link", { name: "Start a request" })).toBeDefined();
  });
});
