import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ProjectDiscoveryRecord,
  ProjectDiscoveryView,
  ProjectSummary,
} from "../coreApi";
import { I18nProvider } from "../i18n";
import { ProjectDetail } from "./ProjectDetail";

const mocks = vi.hoisted(() => ({
  getProjectSummary: vi.fn(),
  getProjectDiscovery: vi.fn(),
  scanProjectDiscovery: vi.fn(),
  updateProjectConfig: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const summary: ProjectSummary = {
  as_of: "2026-10-28T12:00:00Z",
  project: {
    id: "project-1",
    name: "Widgets",
    source_kind: "github",
    source_url: "https://github.com/acme/widgets",
    source_visibility: "private",
    default_branch: "main",
    base_commit: "0123456789abcdef0123456789abcdef01234567",
    created_at: "2026-10-27T10:00:00Z",
    last_activity_at: "2026-10-28T09:00:00Z",
    remote_origin: "https://github.com/acme/widgets.git",
    technical: {
      id: "project-1",
      repository_path: "/projects/project-abc",
      identity: "a".repeat(64),
      source_key: "github:acme/widgets",
      remote_origin: "https://github.com/acme/widgets.git",
    },
  },
  github: {
    health: "CONNECTED",
    connection: {
      configured: true,
      enabled: true,
      login: "acme-bot",
      scopes: [],
      scopes_reported: false,
      credential_fingerprint: "fingerprint",
      revision: 2,
      verified_at: "2026-10-28T10:00:00Z",
      revoked_at: null,
      updated_at: "2026-10-28T10:00:00Z",
      detail: "credential verified",
    },
    binding: {
      project_id: "project-1",
      owner: "acme",
      repo: "widgets",
      repo_full_name: "acme/widgets",
      default_branch: "main",
      repository_github_id: null,
      bound_at: "2026-10-27T10:00:00Z",
      updated_at: "2026-10-27T10:00:00Z",
    },
    sync_state: {
      project_id: "project-1",
      repo_full_name: "acme/widgets",
      status: "OK",
      detail: "Issues synchronized",
      etag: null,
      last_synced_at: "2026-10-28T09:00:00Z",
      last_attempt_at: "2026-10-28T09:00:00Z",
      next_allowed_at: null,
      last_status_code: 200,
      fetched: 2,
      created: 1,
      updated: 1,
      removed: 0,
      total_known: 2,
      truncated: false,
      stale: false,
      updated_at: "2026-10-28T09:00:00Z",
    },
  },
  config: {
    revision: 1,
    config_hash: "b".repeat(64),
    allowed_paths: ["src"],
    capabilities: ["repository-edit"],
    quality_profile: "generic",
    active: true,
    updated_at: "2026-10-28T09:00:00Z",
  },
  campaigns: {
    active: [
      {
        id: "campaign-1",
        actionId: "action-1",
        objective: "Implement the widget",
        status: "AUTHORIZED",
        createdAt: "2026-10-28T08:00:00Z",
      },
    ],
    active_count: 1,
    recent: [],
    total: 1,
  },
  pending_decisions: [
    {
      id: "campaign-authorization:campaign-1",
      kind: "campaign.authorization",
      severity: "attention",
      actionable: true,
      title: "Campaign awaiting authorization",
      detail: "Campaign campaign-1 is blocked.",
      target: "/campaigns/campaign-1",
      subject_id: "campaign-1",
      updated_at: "2026-10-28T08:00:00Z",
    },
  ],
  pending_decision_count: 1,
  tooling: null,
};

const discoveryRecord: ProjectDiscoveryRecord = {
  projectId: "project-1",
  baseCommit: summary.project.base_commit,
  scannedAt: "2026-10-28T11:00:00Z",
  discovery: {
    baseCommit: summary.project.base_commit,
    toolchains: [
      {
        id: "rust",
        label: "Rust (Cargo)",
        manifests: ["Cargo.toml"],
        evidence: ["Cargo manifest detected"],
        suggestedCapability: "generic-toolchain",
        suggestedChecks: [],
      },
    ],
    suggestedCapabilities: ["repository-edit", "generic-toolchain"],
    suggestedChecks: [
      {
        id: "rust-test",
        kind: "test",
        description: "Run the Cargo test suite",
        command: ["cargo", "test", "--all-targets"],
        expectExit: 0,
        timeoutSecs: 300,
      },
    ],
    scannedManifests: ["Cargo.toml"],
    truncated: false,
    skippedEntries: 0,
    provenance: "git-tree-manifest-scan-v1",
    scannedAt: "2026-10-28T11:00:00Z",
  },
};

const noDiscovery: ProjectDiscoveryView = {
  discovery: null,
  current_base_commit: summary.project.base_commit,
  stale: false,
};

function renderDetail() {
  return render(
    <MemoryRouter initialEntries={["/projects/project-1"]}>
      <I18nProvider initialLocale="en">
        <Routes>
          <Route path="/projects/:projectId" element={<ProjectDetail />} />
        </Routes>
      </I18nProvider>
    </MemoryRouter>,
  );
}

describe("ProjectDetail", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "en";
    mocks.getProjectSummary.mockReset().mockResolvedValue(summary);
    mocks.getProjectDiscovery.mockReset().mockResolvedValue(noDiscovery);
    mocks.scanProjectDiscovery.mockReset();
    mocks.updateProjectConfig.mockReset();
  });

  it("renders the Core-backed project summary and technical details", async () => {
    renderDetail();

    expect(
      await screen.findByRole("heading", { name: "Widgets" }),
    ).toBeTruthy();
    expect(screen.getAllByText("acme/widgets").length).toBeGreaterThan(0);
    expect(screen.getAllByText("CONNECTED").length).toBeGreaterThan(0);
    expect(screen.getByText("Active Campaigns: 1")).toBeTruthy();
    expect(screen.getByText("Pending decisions: 1")).toBeTruthy();
    expect(screen.getByText("Campaign awaiting authorization")).toBeTruthy();
    expect(screen.getByText("Toolchain discovery")).toBeTruthy();
    const technical = screen.getByText("Technical details").closest("details");
    expect(technical).not.toBeNull();
    expect(technical?.textContent).toContain("/projects/project-abc");
    expect(technical?.textContent).toContain("github:acme/widgets");
  });

  it("scans bounded manifests and shows advisory checks", async () => {
    mocks.scanProjectDiscovery.mockResolvedValue(discoveryRecord);
    mocks.getProjectSummary.mockResolvedValueOnce(summary).mockResolvedValue({
      ...summary,
      tooling: {
        base_commit: discoveryRecord.baseCommit,
        scanned_at: discoveryRecord.scannedAt,
        stale: false,
        toolchains: discoveryRecord.discovery.toolchains,
        suggested_capabilities: discoveryRecord.discovery.suggestedCapabilities,
        suggested_checks: discoveryRecord.discovery.suggestedChecks,
        truncated: false,
        provenance: discoveryRecord.discovery.provenance,
      },
    });
    const user = userEvent.setup();
    renderDetail();

    await screen.findByRole("heading", { name: "Widgets" });
    await user.click(screen.getByRole("button", { name: "Scan repository" }));
    await waitFor(() =>
      expect(mocks.scanProjectDiscovery).toHaveBeenCalledWith("project-1"),
    );
    expect(await screen.findByText("Rust (Cargo)")).toBeTruthy();
    expect(screen.getByText("Suggested capabilities")).toBeTruthy();
    expect(screen.getByText("cargo test --all-targets")).toBeTruthy();
    expect(screen.getByText(/advisory evidence, not proof/)).toBeTruthy();
  });

  it("applies discovered capabilities with explicit paths and the current revision", async () => {
    mocks.getProjectSummary.mockResolvedValue({
      ...summary,
      tooling: {
        base_commit: discoveryRecord.baseCommit,
        scanned_at: discoveryRecord.scannedAt,
        stale: false,
        toolchains: discoveryRecord.discovery.toolchains,
        suggested_capabilities: discoveryRecord.discovery.suggestedCapabilities,
        suggested_checks: discoveryRecord.discovery.suggestedChecks,
        truncated: false,
        provenance: discoveryRecord.discovery.provenance,
      },
    });
    mocks.getProjectDiscovery.mockResolvedValue({
      discovery: discoveryRecord,
      current_base_commit: summary.project.base_commit,
      stale: false,
    });
    mocks.updateProjectConfig.mockResolvedValue({
      projectId: "project-1",
      revision: 2,
      configHash: "c".repeat(64),
      repository: "/projects/project-abc",
      baseCommit: summary.project.base_commit,
      allowedPaths: ["src"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
      updatedAt: "2026-10-28T12:00:00Z",
    });
    const user = userEvent.setup();
    renderDetail();

    await screen.findByText("Suggested capabilities");
    await user.click(
      screen.getByRole("button", { name: "Apply suggested capabilities" }),
    );
    await waitFor(() =>
      expect(mocks.updateProjectConfig).toHaveBeenCalledWith("project-1", {
        expectedRevision: 1,
        allowedPaths: ["src"],
        capabilities: ["repository-edit", "generic-toolchain"],
        qualityProfile: "generic",
        active: true,
      }),
    );
    expect(
      await screen.findByText(
        "Suggested capabilities were added to the Project configuration revision.",
      ),
    ).toBeTruthy();
  });

  it("renders a recovery action when the summary cannot be loaded", async () => {
    mocks.getProjectSummary.mockRejectedValue(new Error("Core offline"));
    renderDetail();

    expect(await screen.findByText("Project summary unavailable")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Back to Projects" })).toBeTruthy();
  });
});
