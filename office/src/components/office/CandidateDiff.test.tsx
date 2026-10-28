import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CandidateDiffSummary } from "../../coreApi";
import { CandidateDiff } from "./CandidateDiff";

const mocks = vi.hoisted(() => ({
  getCandidateDiff: vi.fn(),
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

const BASE = "a".repeat(40);
const CANDIDATE = "b".repeat(40);
const TREE = "c".repeat(40);

const projection: CandidateDiffSummary = {
  candidate_id: "candidate-1",
  candidate: {
    id: "candidate-1",
    attempt_id: "attempt-1",
    action_spec_sha256: "d".repeat(64),
    commit_sha: CANDIDATE,
    tree_sha: TREE,
    implementer_id: "worker-1",
    created_at: "2026-10-28T10:00:00Z",
    integration: false,
    lineage: [],
  },
  gate: {
    candidate_id: "candidate-1",
    outcome: "ACCEPTED",
    reasons: ["verifier PASS", "reviewer PASS"],
    decided_at: "2026-10-28T10:01:00Z",
  },
  project_id: "project-1",
  campaign_id: "campaign-1",
  commit_sha: CANDIDATE,
  tree_sha: TREE,
  base_commit: BASE,
  base_tree_sha: "e".repeat(40),
  changed_paths: ["src/main.rs", "assets/logo.bin", "assets/huge.txt"],
  changed_paths_truncated: false,
  diff_stat: "3 files changed, 4 insertions(+), 1 deletion(-)",
  files: [
    {
      path: "src/main.rs",
      status: "MODIFIED",
      oldMode: "100644",
      newMode: "100644",
      additions: 2,
      deletions: 1,
      binary: false,
      omitted: null,
      patch:
        "diff --git a/src/main.rs b/src/main.rs\nindex 111..222 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,3 @@\n fn main() {}\n+// changed\n+// again\n-// old\n",
      patchTruncated: false,
      patchBytes: 180,
    },
    {
      path: "assets/logo.bin",
      status: "MODIFIED",
      oldMode: "100644",
      newMode: "100644",
      additions: 0,
      deletions: 0,
      binary: true,
      omitted: "BINARY",
      patchTruncated: false,
      patchBytes: 0,
    },
    {
      path: "assets/huge.txt",
      status: "ADDED",
      oldMode: "000000",
      newMode: "100644",
      additions: 120_000,
      deletions: 0,
      binary: false,
      omitted: "TOO_LARGE",
      patchTruncated: false,
      patchBytes: 0,
    },
  ],
  returned_files: 3,
  files_truncated: false,
  total_additions: 120_002,
  total_deletions: 1,
  patch_bytes: 180,
  patch_truncated: false,
  source:
    "Core read-only Git file diff between the frozen base and the exact Candidate; no execution",
};

describe("CandidateDiff", () => {
  beforeEach(() => {
    mocks.getCandidateDiff.mockReset().mockResolvedValue(projection);
  });

  it("shows exact revisions, statuses and a per-file unified patch", async () => {
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByTestId("candidate-diff");
    expect(mocks.getCandidateDiff).toHaveBeenCalledWith(
      "campaign-1",
      "candidate-1",
    );
    expect(screen.getByText("ACCEPTED")).toBeDefined();
    expect(screen.getByText("3")).toBeDefined();
    expect(screen.getByText("+120002")).toBeDefined();
    expect(
      within(screen.getByLabelText("Changed files")).getByRole("button", {
        name: /main\.rs/,
      }),
    ).toBeDefined();
    expect(screen.getByLabelText("Unified diff for src/main.rs")).toBeDefined();
    expect(screen.getByText("+// changed")).toBeDefined();
    expect(screen.getByText("-// old")).toBeDefined();
  });

  it("renders binary and large-file omissions without inventing a patch", async () => {
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByTestId("candidate-diff");
    await userEvent.click(screen.getByRole("button", { name: /logo\.bin/ }));
    expect(
      screen.getByText("Binary file; Core does not render a text patch."),
    ).toBeDefined();
    await userEvent.click(screen.getByRole("button", { name: /huge\.txt/ }));
    expect(
      screen.getByText(
        "File exceeds Core's 512 KiB text bound; no patch is rendered.",
      ),
    ).toBeDefined();
    expect(
      screen.queryByLabelText("Unified diff for assets/huge.txt"),
    ).toBeNull();
  });

  it("copies a candidate SHA through the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByTestId("candidate-diff");
    await userEvent.click(
      screen.getByRole("button", { name: "Copy candidate commit SHA" }),
    );
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(CANDIDATE));
    expect(screen.getByText("Copied")).toBeDefined();
  });

  it("labels truncated projections and patches", async () => {
    mocks.getCandidateDiff.mockResolvedValueOnce({
      ...projection,
      files_truncated: true,
      patch_truncated: true,
      files: [
        { ...projection.files[0], patchTruncated: true, patchBytes: 4096 },
      ],
      returned_files: 1,
    });
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByTestId("candidate-diff");
    expect(
      screen.getByText(/returned only the first 1 changed files/),
    ).toBeDefined();
    expect(screen.getByText(/At least one patch was truncated/)).toBeDefined();
    expect(
      screen.getByText(/Patch truncated at Core's byte bound/),
    ).toBeDefined();
  });

  it("shows an empty state when Core reports no changed files", async () => {
    mocks.getCandidateDiff.mockResolvedValueOnce({
      ...projection,
      files: [],
      changed_paths: [],
      returned_files: 0,
      total_additions: 0,
      total_deletions: 0,
      patch_bytes: 0,
    });
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByText("No changed files");
  });

  it("renders a typed unavailable error with retry", async () => {
    const { CoreApiError } = await import("../../coreApi");
    mocks.getCandidateDiff.mockRejectedValueOnce(
      new CoreApiError(409, "candidate diff projection is unavailable", {
        kind: "IDENTITY_MISMATCH",
      }),
    );
    render(<CandidateDiff campaignId="campaign-1" candidateId="candidate-1" />);
    await screen.findByText(/Diff unavailable/);
    expect(screen.getByText(/IDENTITY_MISMATCH/)).toBeDefined();
    expect(screen.getByRole("button", { name: "Retry" })).toBeDefined();
  });
});
