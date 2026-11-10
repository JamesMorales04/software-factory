import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgenticSkillRegistryEntry, SkillRefreshStatus } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Skills } from "./Skills";

const mocks = vi.hoisted(() => ({
  listAgenticSkills: vi.fn(),
  getSkillRefreshStatus: vi.fn(),
  runSkillRefresh: vi.fn(),
  reviewAgenticSkill: vi.fn(),
  reviewAgenticSkillLicense: vi.fn(),
  promoteAgenticSkill: vi.fn(),
  revokeAgenticSkill: vi.fn(),
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

function entry(
  overrides: Partial<AgenticSkillRegistryEntry["skill"]> = {},
  reviewDue = false,
): AgenticSkillRegistryEntry {
  return {
    reviewDue,
    skill: {
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
      compatibilityJson: '["rust"]',
      successCount: 1,
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
      ...overrides,
    },
  };
}

function refreshStatus(
  overrides: Partial<SkillRefreshStatus> = {},
): SkillRefreshStatus {
  return {
    running: true,
    intervalSeconds: 86400,
    lastRunAt: "2026-11-09T00:00:00Z",
    nextRunAt: "2026-11-10T00:00:00Z",
    lastOutcome: null,
    lastError: null,
    packs: [],
    due: 0,
    blocked: 0,
    truncated: false,
    projectOffset: 0,
    projectsChecked: 0,
    projectsSkipped: 0,
    projectTotal: 0,
    ...overrides,
  };
}

function renderSkills() {
  return render(
    <I18nProvider initialLocale="en">
      <Skills />
    </I18nProvider>,
  );
}

describe("Skills", () => {
  beforeEach(() => {
    mocks.listAgenticSkills
      .mockReset()
      .mockResolvedValue({ skills: [entry()] });
    mocks.getSkillRefreshStatus.mockReset().mockResolvedValue(refreshStatus());
    mocks.runSkillRefresh.mockReset().mockResolvedValue({
      report: {
        startedAt: "2026-11-09T00:00:00Z",
        finishedAt: "2026-11-09T00:00:01Z",
        checked: 1,
        projectsChecked: 0,
        projectsSkipped: 0,
        projectTotal: 0,
        refreshed: [],
        due: [],
        blocked: [],
        failed: [],
        ok: true,
      },
    });
    mocks.reviewAgenticSkill.mockReset().mockResolvedValue({});
    mocks.reviewAgenticSkillLicense.mockReset().mockResolvedValue({});
    mocks.promoteAgenticSkill.mockReset().mockResolvedValue({});
    mocks.revokeAgenticSkill.mockReset().mockResolvedValue({});
  });

  it("lists a temporary skill with its source and review state", async () => {
    renderSkills();
    expect(await screen.findByText("context7-chrono")).toBeDefined();
    expect(screen.getByText("RUN")).toBeDefined();
    expect(screen.getByRole("link", { name: "source" })).toBeDefined();
  });

  it("records a license review and a weekly review", async () => {
    renderSkills();
    await screen.findByText("context7-chrono");
    await userEvent.type(
      screen.getByLabelText("Reviewed upstream license"),
      "MIT",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Record review" }),
    );
    expect(mocks.reviewAgenticSkillLicense).toHaveBeenCalledWith(
      "skill-1",
      "MIT",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Weekly review" }),
    );
    expect(mocks.reviewAgenticSkill).toHaveBeenCalledWith("skill-1");
  });

  it("requires an owner decision before GLOBAL promotion", async () => {
    mocks.listAgenticSkills.mockResolvedValue({
      skills: [
        entry({
          scope: "PROJECT",
          status: "ACTIVE",
          successCount: 2,
          licenseReviewedAt: "2026-11-09T00:00:00Z",
        }),
      ],
    });
    renderSkills();
    await screen.findByText("context7-chrono");
    const promote = screen.getByRole("button", { name: "Promote to GLOBAL" });
    expect((promote as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I am the owner and approve GLOBAL promotion/,
      }),
    );
    await userEvent.click(promote);
    expect(mocks.promoteAgenticSkill).toHaveBeenCalledWith("skill-1", {
      scope: "GLOBAL",
      owner_approved: true,
      confirm: true,
    });
  });

  it("revokes a skill with a reason", async () => {
    renderSkills();
    await screen.findByText("context7-chrono");
    await userEvent.type(
      screen.getByLabelText("Revocation reason"),
      "superseded",
    );
    await userEvent.click(screen.getByRole("button", { name: "Revoke" }));
    expect(mocks.revokeAgenticSkill).toHaveBeenCalledWith(
      "skill-1",
      "superseded",
    );
  });

  it("shows due and blocked technology packs and runs a refresh pass", async () => {
    mocks.getSkillRefreshStatus.mockResolvedValue(
      refreshStatus({
        due: 1,
        blocked: 1,
        packs: [
          {
            technology: "rust",
            state: "REFRESHABLE",
            blocking: false,
            persisted: {
              version: "current",
              contentSha256: "a".repeat(64),
              sourceCheckedAt: "2026-01-01T00:00:00Z",
              contentReviewedAt: "2026-01-01T00:00:00Z",
              reviewBasis: "CONTENT_REVIEWED",
              reviewAfter: "2026-04-01T00:00:00Z",
            },
            builtin: {
              version: "current",
              contentSha256: "b".repeat(64),
              sourceCheckedAt: "2026-11-09T00:00:00Z",
              contentReviewedAt: "2026-11-09T00:00:00Z",
              reviewBasis: "CONTENT_REVIEWED",
              reviewAfter: "2027-02-07T00:00:00Z",
            },
            reason: "the persisted revision is due",
          },
          {
            technology: "legacy-tool",
            state: "STALE",
            blocking: true,
            persisted: null,
            builtin: null,
            reason: "no source exists",
          },
        ],
      }),
    );
    mocks.runSkillRefresh.mockResolvedValue({
      report: {
        startedAt: "2026-11-09T00:00:00Z",
        finishedAt: "2026-11-09T00:00:01Z",
        checked: 2,
        projectsChecked: 0,
        projectsSkipped: 0,
        projectTotal: 0,
        refreshed: ["rust"],
        due: ["rust"],
        blocked: ["legacy-tool"],
        failed: [],
        ok: true,
      },
    });
    renderSkills();
    expect(await screen.findByText("rust")).toBeDefined();
    expect(screen.getByText("REFRESHABLE")).toBeDefined();
    expect(screen.getByText("STALE")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Run refresh pass" }),
    );
    expect(mocks.runSkillRefresh).toHaveBeenCalled();
    expect(
      await screen.findByText("Refresh pass complete: 1 refreshed, 1 blocked."),
    ).toBeDefined();
  });

  it("shows a loading state while refresh status is pending", async () => {
    mocks.getSkillRefreshStatus.mockReturnValue(new Promise(() => {}));
    renderSkills();
    expect(await screen.findByText("Loading skills")).toBeDefined();
  });

  it("shows an error when refresh status cannot be loaded", async () => {
    mocks.getSkillRefreshStatus.mockRejectedValue(new Error("status down"));
    renderSkills();
    expect(
      await screen.findByText("The skill registry could not be loaded."),
    ).toBeDefined();
  });

  it("reports a failed refresh action", async () => {
    mocks.runSkillRefresh.mockRejectedValue(new Error("refresh failed"));
    renderSkills();
    await screen.findByText("context7-chrono");
    await userEvent.click(
      screen.getByRole("button", { name: "Run refresh pass" }),
    );
    expect(await screen.findByText("The skill action failed.")).toBeDefined();
  });

  it("explains that source-only packs require content review", async () => {
    mocks.getSkillRefreshStatus.mockResolvedValue(
      refreshStatus({
        blocked: 1,
        packs: [
          {
            technology: "angular",
            state: "STALE",
            blocking: true,
            persisted: null,
            builtin: {
              version: "current",
              contentSha256: "c".repeat(64),
              sourceCheckedAt: "2026-11-09T00:00:00Z",
              contentReviewedAt: "",
              reviewBasis: "SOURCE_AVAILABILITY_ONLY",
              reviewAfter: "",
            },
            reason:
              "the builtin pack has source availability evidence only; content review is required",
          },
        ],
      }),
    );
    renderSkills();
    expect(await screen.findByText("angular")).toBeDefined();
    expect(
      screen.getByText(
        "the builtin pack has source availability evidence only; content review is required",
      ),
    ).toBeDefined();
    expect(
      screen.getByText("Source only; content review required"),
    ).toBeDefined();
  });

  it("shows truncated technologies and deferred projects", async () => {
    mocks.getSkillRefreshStatus.mockResolvedValue(
      refreshStatus({
        truncated: true,
        projectsChecked: 64,
        projectsSkipped: 66,
        projectTotal: 130,
      }),
    );
    renderSkills();
    expect(await screen.findByText("context7-chrono")).toBeDefined();
    expect(
      screen.getByText(
        "Only the first 256 technologies are shown; the list is bounded.",
      ),
    ).toBeDefined();
    expect(
      screen.getByText(
        "The next pass will scan 64 of 130 projects; 66 will be deferred.",
      ),
    ).toBeDefined();
  });

  it("distinguishes the next scan window from the last completed pass", async () => {
    mocks.getSkillRefreshStatus.mockResolvedValue(
      refreshStatus({
        projectsChecked: 6,
        projectsSkipped: 64,
        projectTotal: 70,
        lastOutcome: {
          startedAt: "2026-11-09T00:00:00Z",
          finishedAt: "2026-11-09T00:00:01Z",
          checked: 70,
          projectsChecked: 64,
          projectsSkipped: 6,
          projectTotal: 70,
          refreshed: [],
          due: [],
          blocked: [],
          failed: [],
          ok: true,
        },
      }),
    );
    renderSkills();
    expect(await screen.findByText("context7-chrono")).toBeDefined();
    expect(
      screen.getByText(
        "The next pass will scan 6 of 70 projects; 64 will be deferred.",
      ),
    ).toBeDefined();
    expect(
      screen.getByText(
        "Last completed pass scanned 64 of 70 projects; 6 deferred.",
      ),
    ).toBeDefined();
  });

  it("shows review due state and an empty registry", async () => {
    mocks.listAgenticSkills.mockResolvedValue({ skills: [] });
    renderSkills();
    expect(await screen.findByText("No skills recorded")).toBeDefined();
  });
});
