import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  CandidatePullRequestStatus,
  ProjectGitHubBinding,
} from "../../coreApi";
import { CandidatePullRequest } from "./CandidatePullRequest";

const mocks = vi.hoisted(() => ({
  getProjectGitHub: vi.fn(),
  getCandidatePullRequest: vi.fn(),
  getCandidatePullRequestComments: vi.fn(),
  getCandidatePullRequestDiff: vi.fn(),
  commentOnCandidatePullRequest: vi.fn(),
  requestCandidatePullRequestReview: vi.fn(),
  mergeCandidatePullRequest: vi.fn(),
  openCandidatePullRequest: vi.fn(),
}));

vi.mock("../../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    status: number;
    body: unknown;
    constructor(status: number, message: string, body?: unknown) {
      super(message);
      this.status = status;
      this.body = body ?? null;
    }
  },
}));

const HEAD = "a".repeat(40);
const BASE = "b".repeat(40);

const binding: ProjectGitHubBinding = {
  project_id: "project-1",
  owner: "acme",
  repo: "widgets",
  repo_full_name: "acme/widgets",
  default_branch: "main",
  bound_at: "2026-10-28T09:00:00Z",
  updated_at: "2026-10-28T09:00:00Z",
};

const status: CandidatePullRequestStatus = {
  campaign_id: "campaign-1",
  candidate_id: "candidate-1",
  candidate_commit_sha: HEAD,
  candidate_tree_sha: "c".repeat(40),
  project_id: "project-1",
  gate: {
    candidate_id: "candidate-1",
    outcome: "ACCEPTED",
    reasons: [],
    decided_at: "2026-10-28T10:00:00Z",
  },
  binding,
  pull_request: {
    number: 42,
    title: "Candidate 42",
    state: "open",
    draft: false,
    merged: false,
    mergeable: true,
    mergeableState: "clean",
    headRef: "candidate/office-ux",
    headSha: HEAD,
    baseRef: "main",
    baseSha: BASE,
    author: "octo",
    htmlUrl: "https://github.com/acme/widgets/pull/42",
    changedFiles: 2,
    additions: 10,
    deletions: 2,
    comments: 1,
    createdAt: "2026-10-28T09:30:00Z",
    updatedAt: "2026-10-28T09:40:00Z",
  },
  checks: {
    headSha: HEAD,
    state: "SUCCESS",
    detail: "all reported checks succeeded",
    ready: true,
    checkRuns: [
      {
        id: 1,
        name: "test",
        status: "completed",
        conclusion: "success",
        app: "github-actions",
        detailsUrl: "https://github.com/acme/widgets/actions/1",
        startedAt: null,
        completedAt: null,
      },
    ],
    checkRunsTruncated: false,
    combinedStatus: "success",
    statuses: [],
    statusesTruncated: false,
    fetchedAt: "2026-10-28T10:00:00Z",
  },
  head_matches_candidate: true,
  merge_ready: true,
  merge_blocked_reasons: [],
  checked_at: "2026-10-28T10:00:00Z",
  source: "GitHub REST read through Core",
};

const comments = {
  campaign_id: "campaign-1",
  candidate_id: "candidate-1",
  project_id: "project-1",
  binding,
  pull_request_number: 42,
  comments: {
    comments: [
      {
        id: 1,
        kind: "issue",
        author: "reviewer",
        body: "Please confirm the bounded diff.",
        bodyTruncated: false,
        path: null,
        line: null,
        htmlUrl: "https://github.com/acme/widgets/pull/42#issuecomment-1",
        createdAt: "2026-10-28T09:50:00Z",
      },
    ],
    truncated: false,
    fetchedAt: "2026-10-28T10:00:00Z",
  },
  source: "GitHub REST read through Core",
};

async function renderBound() {
  render(
    <CandidatePullRequest
      campaignId="campaign-1"
      candidateId="candidate-1"
      projectId="project-1"
      candidateCommitSha={HEAD}
    />,
  );
  await screen.findByText("acme/widgets");
}

async function readPullRequest() {
  await userEvent.type(screen.getByLabelText("Pull request number"), "42");
  await userEvent.click(
    screen.getByRole("button", { name: "Read PR and checks" }),
  );
  await screen.findByTestId("pr-status");
}

describe("CandidatePullRequest", () => {
  beforeEach(() => {
    mocks.getProjectGitHub.mockReset().mockResolvedValue({
      binding,
      sync_state: null,
    });
    mocks.getCandidatePullRequest.mockReset().mockResolvedValue(status);
    mocks.getCandidatePullRequestComments
      .mockReset()
      .mockResolvedValue(comments);
    mocks.getCandidatePullRequestDiff.mockReset();
    mocks.commentOnCandidatePullRequest.mockReset();
    mocks.requestCandidatePullRequestReview.mockReset();
    mocks.mergeCandidatePullRequest.mockReset();
    mocks.openCandidatePullRequest.mockReset();
  });

  it("reads a pull request and checks, and never merges without confirmation", async () => {
    await renderBound();
    await readPullRequest();
    expect(mocks.getCandidatePullRequest).toHaveBeenCalledWith(
      "campaign-1",
      "candidate-1",
      42,
    );
    expect(screen.getByText("candidate/office-ux")).toBeDefined();
    expect(screen.getByText("test")).toBeDefined();
    expect(screen.getByText(/all reported checks succeeded/)).toBeDefined();
    expect(screen.getByText("matches Candidate")).toBeDefined();

    const merge = screen.getByRole("button", { name: "Merge pull request" });
    expect((merge as HTMLButtonElement).disabled).toBe(true);
    expect(mocks.mergeCandidatePullRequest).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByLabelText(
        "I explicitly confirm this merge of the exact expected head.",
      ),
    );
    expect((merge as HTMLButtonElement).disabled).toBe(false);
    expect(mocks.mergeCandidatePullRequest).not.toHaveBeenCalled();
  });

  it("submits a merge with the exact expected head only after confirmation", async () => {
    mocks.mergeCandidatePullRequest.mockResolvedValue({
      campaign_id: "campaign-1",
      candidate_id: "candidate-1",
      project_id: "project-1",
      pull_request_number: 42,
      expected_head_sha: HEAD,
      merged: true,
      merge_commit_sha: "d".repeat(40),
      message: "Pull Request successfully merged",
      merged_at: "2026-10-28T10:02:00Z",
      source: "merge",
    });
    await renderBound();
    await readPullRequest();
    await userEvent.click(
      screen.getByLabelText(
        "I explicitly confirm this merge of the exact expected head.",
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Merge pull request" }),
    );
    await waitFor(() =>
      expect(mocks.mergeCandidatePullRequest).toHaveBeenCalledWith(
        "campaign-1",
        "candidate-1",
        {
          number: 42,
          expected_head_sha: HEAD,
          merge_method: "squash",
          confirmed: true,
        },
      ),
    );
  });

  it("shows Core's fresh-read merge blockers and disables merge", async () => {
    mocks.getCandidatePullRequest.mockResolvedValue({
      ...status,
      merge_ready: false,
      merge_blocked_reasons: [
        "GitHub merge state is blocked (branch protection or required checks are not satisfied)",
        "checks are FAILURE: at least one reported check failed",
      ],
      checks: { ...status.checks, state: "FAILURE", ready: false },
    });
    await renderBound();
    await readPullRequest();
    expect(
      screen.getByText(/Merge blocked by Core's fresh read/),
    ).toBeDefined();
    expect(screen.getByText(/GitHub merge state is blocked/)).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Merge pull request",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("renders an unbound Project without inventing a repository", async () => {
    mocks.getProjectGitHub.mockResolvedValue({
      binding: null,
      sync_state: null,
    });
    render(
      <CandidatePullRequest
        campaignId="campaign-1"
        candidateId="candidate-1"
        projectId="project-1"
        candidateCommitSha={HEAD}
      />,
    );
    await screen.findByText(/not bound to a GitHub repository/);
    expect(
      screen.queryByRole("button", { name: "Read PR and checks" }),
    ).toBeNull();
  });

  it("shows a typed permission failure instead of a fake PR", async () => {
    const { CoreApiError } = await import("../../coreApi");
    mocks.getCandidatePullRequest.mockRejectedValueOnce(
      new CoreApiError(403, "GitHub denied repository access", {
        kind: "DENIED",
      }),
    );
    await renderBound();
    await userEvent.type(screen.getByLabelText("Pull request number"), "42");
    await userEvent.click(
      screen.getByRole("button", { name: "Read PR and checks" }),
    );
    await screen.findByText(/GitHub denied repository access/);
    expect(screen.getByText(/DENIED/)).toBeDefined();
    expect(screen.queryByTestId("pr-status")).toBeNull();
  });

  it("posts a comment through Core and refreshes the conversation", async () => {
    mocks.commentOnCandidatePullRequest.mockResolvedValue({
      campaign_id: "campaign-1",
      candidate_id: "candidate-1",
      project_id: "project-1",
      pull_request_number: 42,
      comment: comments.comments.comments[0],
      recorded_at: "2026-10-28T10:03:00Z",
      source: "comment",
    });
    await renderBound();
    await readPullRequest();
    await userEvent.type(
      screen.getByLabelText("Add a comment"),
      "Looks bounded and exact.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Post comment" }));
    await waitFor(() =>
      expect(mocks.commentOnCandidatePullRequest).toHaveBeenCalledWith(
        "campaign-1",
        "candidate-1",
        42,
        "Looks bounded and exact.",
      ),
    );
    expect(
      await screen.findByText(/Comment accepted by GitHub through Core/),
    ).toBeDefined();
  });

  it("labels a head-branch mismatch when opening a PR", async () => {
    const { CoreApiError } = await import("../../coreApi");
    mocks.openCandidatePullRequest.mockRejectedValueOnce(
      new CoreApiError(
        409,
        "GitHub head branch does not resolve to the exact candidate commit",
        { kind: "HEAD_MISMATCH" },
      ),
    );
    await renderBound();
    await userEvent.click(
      screen.getByText("Open a pull request from an exact branch"),
    );
    await userEvent.type(
      screen.getByLabelText("Head branch"),
      "candidate/office-ux",
    );
    await userEvent.type(screen.getByLabelText("Title"), "Candidate 42");
    await userEvent.click(
      screen.getByRole("button", { name: "Open pull request" }),
    );
    await screen.findByText(/HEAD_MISMATCH/);
    expect(mocks.openCandidatePullRequest).toHaveBeenCalledWith(
      "campaign-1",
      "candidate-1",
      expect.objectContaining({ head: "candidate/office-ux" }),
    );
  });
});
