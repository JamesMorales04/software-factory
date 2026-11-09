import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgenticKnowledgeResponse, AgenticSkillRecord } from "../coreApi";
import { I18nProvider } from "../i18n";
import { KnowledgePanel } from "./KnowledgePanel";

const mocks = vi.hoisted(() => ({
  getAgenticKnowledge: vi.fn(),
  researchAgenticKnowledge: vi.fn(),
  adoptAgenticSkill: vi.fn(),
  recordAgenticSkillUse: vi.fn(),
  reviewAgenticSkillLicense: vi.fn(),
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

const skill: AgenticSkillRecord = {
  id: "skill-1",
  projectId: "project-1",
  requestId: "request-1",
  name: "context7-chrono",
  scope: "RUN",
  status: "ACTIVE",
  manifestJson: JSON.stringify({
    sourceUrl: "https://context7.com/chronotope/chrono",
  }),
  manifestSha256: "a".repeat(64),
  provenance: "CONTEXT7_MCP:/chronotope/chrono",
  license: "NOASSERTION",
  licenseReviewedAt: null,
  compatibilityJson: '["rust","chrono"]',
  successCount: 0,
  failureCount: 0,
  lastUsedAt: null,
  promotedAt: null,
  lastReviewedAt: null,
  nextReviewAt: null,
  revokedAt: null,
  ownerApprovedBy: null,
  expiresAt: "2026-11-16T00:00:00Z",
  createdAt: "2026-11-09T00:00:00Z",
  updatedAt: "2026-11-09T00:00:00Z",
};

const empty: AgenticKnowledgeResponse = {
  pack: null,
  skills: [],
  adoptions: [],
  inventory: {
    manifests: ["Cargo.toml"],
    libraries: [{ name: "serde", version: "1", manifest: "Cargo.toml" }],
    evidenceRefs: ["Cargo.toml"],
  },
};

const withPack: AgenticKnowledgeResponse = {
  pack: {
    id: "pack-1",
    requestId: "request-1",
    projectId: "project-1",
    library: "chrono",
    libraryId: "/chronotope/chrono",
    query: "timezone-aware export",
    packJson: JSON.stringify({
      library: "chrono",
      libraryId: "/chronotope/chrono",
      query: "timezone-aware export",
      sources: [
        {
          title: "Chrono — Context7",
          url: "https://context7.com/chronotope/chrono",
          kind: "CONTEXT7_MCP",
          snippet: "Use Utc::now().to_rfc3339().",
          reputation: "High",
          benchmarkScore: 77.3,
        },
      ],
      packSha256: "b".repeat(64),
      fetchedAt: "2026-11-09T00:00:00Z",
    }),
    packSha256: "b".repeat(64),
    provider: "CONTEXT7_MCP",
    sourceUrl: "https://context7.com/chronotope/chrono",
    fetchedAt: "2026-11-09T00:00:00Z",
    createdAt: "2026-11-09T00:00:00Z",
  },
  skills: [skill],
  adoptions: [],
  inventory: empty.inventory,
};

function renderPanel() {
  return render(
    <I18nProvider initialLocale="en">
      <MemoryRouter>
        <KnowledgePanel requestId="request-1" />
      </MemoryRouter>
    </I18nProvider>,
  );
}

describe("KnowledgePanel", () => {
  beforeEach(() => {
    mocks.getAgenticKnowledge.mockReset().mockResolvedValue(empty);
    mocks.researchAgenticKnowledge.mockReset().mockResolvedValue({});
    mocks.adoptAgenticSkill.mockReset().mockResolvedValue({});
    mocks.recordAgenticSkillUse.mockReset().mockResolvedValue({});
    mocks.reviewAgenticSkillLicense.mockReset().mockResolvedValue({});
  });

  it("shows Explorer evidence and researches an evidenced gap", async () => {
    renderPanel();
    expect(await screen.findByText(/Manifests: Cargo.toml/)).toBeDefined();
    await userEvent.type(screen.getByLabelText("Library"), "chrono");
    await userEvent.type(
      screen.getByLabelText("Documentation query"),
      "timezone-aware export",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Research with Context7" }),
    );
    expect(mocks.researchAgenticKnowledge).toHaveBeenCalledWith(
      "request-1",
      "chrono",
      "timezone-aware export",
    );
  });

  it("shows the KnowledgePack, adopts the skill and records a successful use", async () => {
    mocks.getAgenticKnowledge.mockResolvedValue(withPack);
    renderPanel();
    expect(await screen.findByText("Chrono — Context7")).toBeDefined();
    expect(screen.getByText(/Use Utc::now/)).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Adopt for this operation" }),
    );
    expect(mocks.adoptAgenticSkill).toHaveBeenCalledWith(
      "request-1",
      "skill-1",
    );
    await userEvent.type(
      screen.getByLabelText("Use evidence"),
      "candidate accepted",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Record successful use" }),
    );
    expect(mocks.recordAgenticSkillUse).toHaveBeenCalledWith("skill-1", {
      request_id: "request-1",
      outcome: "SUCCESS",
      evidence: "candidate accepted",
    });
  });

  it("requires evidence before recording a use", async () => {
    mocks.getAgenticKnowledge.mockResolvedValue(withPack);
    renderPanel();
    await screen.findByText("Chrono — Context7");
    await userEvent.click(
      screen.getByRole("button", { name: "Record failed use" }),
    );
    expect(mocks.recordAgenticSkillUse).not.toHaveBeenCalled();
    expect(
      await screen.findByText("Evidence is required to record a skill use."),
    ).toBeDefined();
  });
});
