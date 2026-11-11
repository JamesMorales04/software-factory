import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  DeliveryAttempt,
  DeliveryIncident,
  DeliveryPolicy,
  DeliveryStatus,
} from "../../coreApi";
import { I18nProvider } from "../../i18n";
import { CampaignDelivery } from "./CampaignDelivery";

const mocks = vi.hoisted(() => ({
  getCandidateDelivery: vi.fn(),
  deliverCandidate: vi.fn(),
  recordDeliveryRevert: vi.fn(),
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
const MERGE = "e".repeat(40);

const policy: DeliveryPolicy = {
  schemaVersion: 1,
  projectId: "project-1",
  revision: 3,
  policySha256: "p".repeat(64),
  mode: "RISK_TIERED",
  r1Enabled: true,
  autoMergeBaseBranch: "main",
  manualBaseBranches: ["main"],
  requiredChecks: ["ci"],
  mergeMethod: "squash",
  postMergeCheckRequired: true,
  postMergeCheckId: "post-merge",
  postMergeTimeoutSecs: 900,
  killSwitch: false,
  createdBy: "owner",
  createdAt: "2026-11-01T00:00:00Z",
  expiresAt: null,
};

function attempt(overrides: Partial<DeliveryAttempt> = {}): DeliveryAttempt {
  return {
    attemptId: "attempt-1",
    idempotencyKey: "key",
    schemaVersion: 1,
    operation: "MERGE",
    projectId: "project-1",
    campaignId: "campaign-1",
    candidateId: "candidate-1",
    candidateSha: HEAD,
    candidateTreeSha: "b".repeat(40),
    contentDigest: "c".repeat(64),
    gateRevision: "gate-1",
    gateOutcome: "ACCEPTED",
    policyRevision: 3,
    policySha256: "p".repeat(64),
    riskClass: "R0",
    authorization: "POLICY_R0",
    charterGrantId: null,
    pullRequestNumber: 42,
    baseBranch: "main",
    expectedHeadSha: HEAD,
    observedHeadSha: HEAD,
    observedBaseSha: "d".repeat(40),
    mergeMethod: "squash",
    state: "MERGED",
    mergeCommitSha: MERGE,
    incidentId: null,
    postMergeCheckId: null,
    blockerCode: null,
    blockerDetail: null,
    createdAt: "2026-11-02T00:00:00Z",
    updatedAt: "2026-11-02T00:01:00Z",
    mergedAt: "2026-11-02T00:01:00Z",
    ...overrides,
  };
}

function incident(overrides: Partial<DeliveryIncident> = {}): DeliveryIncident {
  return {
    incidentId: "incident-1",
    projectId: "project-1",
    attemptId: "attempt-1",
    candidateId: "candidate-1",
    mergeCommitSha: MERGE,
    kind: "POST_MERGE_CHECK_FAILED",
    state: "OPEN",
    detail: "post-merge check failed",
    createdAt: "2026-11-02T00:02:00Z",
    updatedAt: "2026-11-02T00:02:00Z",
    acknowledgedBy: null,
    acknowledgedAt: null,
    resolvedAt: null,
    ...overrides,
  };
}

function status(overrides: Partial<DeliveryStatus> = {}): DeliveryStatus {
  return {
    campaignId: "campaign-1",
    candidateId: "candidate-1",
    candidateSha: HEAD,
    projectId: "project-1",
    riskClass: "R0",
    finalSource: "MODEL_FINAL",
    salvageRequiresHumanReview: false,
    salvageEvidence: null,
    salvageError: null,
    gateOutcome: "ACCEPTED",
    gateRevision: "gate-1",
    policy,
    binding: {
      project_id: "project-1",
      owner: "acme",
      repo: "widgets",
      repo_full_name: "acme/widgets",
      default_branch: "main",
      repository_github_id: null,
      bound_at: "2026-11-01T00:00:00Z",
      updated_at: "2026-11-01T00:00:00Z",
    },
    projection: {
      policy,
      attempts: [],
      decisions: [],
      checkReceipts: [],
      postMergeChecks: [],
      incidents: [],
      reverts: [],
      issueReceipts: [],
    },
    pullRequest: {
      number: 42,
      title: "Factory candidate",
      state: "open",
      draft: false,
      merged: false,
      mergeable: true,
      mergeableState: "clean",
      headRef: "factory/candidate",
      headSha: HEAD,
      baseRef: "main",
      baseSha: "d".repeat(40),
      author: "factory",
      htmlUrl: "https://github.com/acme/widgets/pull/42",
      changedFiles: 1,
      additions: 1,
      deletions: 0,
      comments: 0,
      createdAt: "2026-11-02T00:00:00Z",
      updatedAt: "2026-11-02T00:00:00Z",
    },
    checks: {
      headSha: HEAD,
      state: "SUCCESS",
      detail: "all reported checks succeeded",
      ready: true,
      checkRuns: [
        {
          id: 1,
          name: "ci",
          status: "completed",
          conclusion: "success",
          app: "actions",
          detailsUrl: "",
          startedAt: null,
          completedAt: null,
        },
      ],
      checkRunsTruncated: false,
      combinedStatus: "success",
      statuses: [],
      statusesTruncated: false,
      fetchedAt: "2026-11-02T00:00:00Z",
    },
    protection: {
      protected: true,
      requiredStatusChecksStrict: true,
      requiredChecks: ["ci"],
      requiredPullRequestReviews: false,
      requiredApprovingReviewCount: 0,
      enforceAdmins: false,
      allowForcePushes: false,
      allowDeletions: false,
      requiredConversationResolution: false,
      fetchedAt: "2026-11-02T00:00:00Z",
    },
    eligibility: null,
    liveError: null,
    ...overrides,
  };
}

function renderPanel() {
  render(
    <I18nProvider>
      <CampaignDelivery campaignId="campaign-1" candidateId="candidate-1" />
    </I18nProvider>,
  );
}

describe("CampaignDelivery", () => {
  beforeEach(() => {
    mocks.getCandidateDelivery.mockReset();
    mocks.deliverCandidate.mockReset();
    mocks.recordDeliveryRevert.mockReset();
  });

  it("renders the policy, checks, blockers and performs a Core delivery", async () => {
    mocks.getCandidateDelivery
      .mockResolvedValueOnce(
        status({
          projection: {
            ...status().projection,
            attempts: [
              attempt({
                state: "BLOCKED",
                mergeCommitSha: null,
                mergedAt: null,
                blockerCode: "HEAD_MISMATCH",
                blockerDetail: "stale head",
              }),
            ],
            decisions: [
              {
                decisionId: "decision-1",
                attemptId: "attempt-1",
                candidateId: "candidate-1",
                candidateSha: HEAD,
                gateRevision: "gate-1",
                gateOutcome: "ACCEPTED",
                policyRevision: 3,
                policySha256: "p".repeat(64),
                charterGrantId: null,
                riskClass: "R0",
                expectedHeadSha: HEAD,
                baseBranch: "main",
                checksReadAt: "2026-11-02T00:00:00Z",
                branchProtectionReadAt: "2026-11-02T00:00:00Z",
                decision: "DENY",
                reasons: [{ code: "HEAD_MISMATCH", detail: "stale head" }],
                decidedAt: "2026-11-02T00:00:00Z",
              },
            ],
          },
        }),
      )
      .mockResolvedValueOnce(status());
    mocks.deliverCandidate.mockResolvedValue({
      attempt: attempt(),
      policy,
      decision: {},
      pullRequest: null,
      checks: null,
      protection: null,
      issueOutcome: null,
      replayed: false,
    });

    const user = userEvent.setup();
    renderPanel();

    expect(await screen.findByText("Delivery")).toBeDefined();
    expect(screen.getByText("R0 automatic")).toBeDefined();
    expect(screen.getByText(/HEAD_MISMATCH/)).toBeDefined();
    expect(
      screen.getAllByText(/all reported checks succeeded|ci/).length,
    ).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Deliver candidate" }));
    await waitFor(() =>
      expect(mocks.deliverCandidate).toHaveBeenCalledWith(
        "campaign-1",
        "candidate-1",
        { operation: "MERGE", manual_confirmed: false },
      ),
    );
    expect(await screen.findByText(/Core recorded the merge/)).toBeDefined();
    expect(mocks.getCandidateDelivery).toHaveBeenCalledTimes(2);
  });

  it("requires an explicit owner decision for manual delivery", async () => {
    mocks.getCandidateDelivery.mockResolvedValue(
      status({
        riskClass: "R3",
        policy: { ...policy, mode: "MANUAL", r1Enabled: false },
        projection: {
          policy: { ...policy, mode: "MANUAL", r1Enabled: false },
          attempts: [],
          decisions: [],
          checkReceipts: [],
          postMergeChecks: [],
          incidents: [],
          reverts: [],
          issueReceipts: [],
        },
      }),
    );
    mocks.deliverCandidate.mockResolvedValue({
      attempt: attempt({ riskClass: "R3", authorization: "OWNER_MANUAL" }),
      policy: { ...policy, mode: "MANUAL", r1Enabled: false },
      decision: {},
      pullRequest: null,
      checks: null,
      protection: null,
      issueOutcome: null,
      replayed: false,
    });

    const user = userEvent.setup();
    renderPanel();
    const deliver = await screen.findByRole("button", {
      name: "Deliver candidate",
    });
    expect((deliver as HTMLButtonElement).disabled).toBe(true);
    await user.click(
      screen.getByRole("checkbox", {
        name: "I am the owner and approve this exact merge",
      }),
    );
    expect((deliver as HTMLButtonElement).disabled).toBe(false);
    await user.click(deliver);
    await waitFor(() =>
      expect(mocks.deliverCandidate).toHaveBeenCalledWith(
        "campaign-1",
        "candidate-1",
        { operation: "MERGE", manual_confirmed: true },
      ),
    );
  });

  it("shows an incident, requires a manual revert and records its evidence", async () => {
    mocks.getCandidateDelivery
      .mockResolvedValueOnce(
        status({
          projection: {
            ...status().projection,
            attempts: [
              attempt({ state: "NEEDS_ATTENTION", incidentId: "incident-1" }),
            ],
            incidents: [incident()],
          },
        }),
      )
      .mockResolvedValueOnce(
        status({
          projection: {
            ...status().projection,
            attempts: [
              attempt({ state: "NEEDS_ATTENTION", incidentId: "incident-1" }),
            ],
            incidents: [incident({ state: "REVERTED" })],
            reverts: [
              {
                revertId: "revert-1",
                incidentId: "incident-1",
                revertPrNumber: 77,
                revertMergeSha: "f".repeat(40),
                actor: "operator-token",
                rationale: "manual revert",
                createdAt: "2026-11-02T00:05:00Z",
              },
            ],
          },
        }),
      );
    mocks.recordDeliveryRevert.mockResolvedValue({
      revert: {
        revertId: "revert-1",
        incidentId: "incident-1",
        revertPrNumber: 77,
        revertMergeSha: "f".repeat(40),
        actor: "operator-token",
        rationale: "manual revert",
        createdAt: "2026-11-02T00:05:00Z",
      },
      source: "test",
    });

    const user = userEvent.setup();
    renderPanel();
    expect(await screen.findByText("Delivery freeze")).toBeDefined();
    expect(
      screen.getAllByText(/Factory does not auto-revert/).length,
    ).toBeGreaterThan(0);
    await user.type(screen.getByLabelText("Revert pull request number"), "77");
    await user.type(
      screen.getByLabelText("Revert merge commit SHA"),
      "f".repeat(40),
    );
    await user.type(screen.getByLabelText("Revert rationale"), "manual revert");
    await user.click(
      screen.getByRole("button", { name: "Record manual revert" }),
    );
    await waitFor(() =>
      expect(mocks.recordDeliveryRevert).toHaveBeenCalledWith("incident-1", {
        revert_pr_number: 77,
        revert_merge_sha: "f".repeat(40),
        rationale: "manual revert",
      }),
    );
    expect(
      await screen.findByText(/Manual revert evidence recorded/),
    ).toBeDefined();
  });

  it("renders a truthful empty policy state without decorative actions", async () => {
    mocks.getCandidateDelivery.mockResolvedValue(
      status({
        policy: null,
        binding: null,
        pullRequest: null,
        checks: null,
        protection: null,
        projection: {
          policy: null,
          attempts: [],
          decisions: [],
          checkReceipts: [],
          postMergeChecks: [],
          incidents: [],
          reverts: [],
          issueReceipts: [],
        },
      }),
    );
    renderPanel();
    expect(
      await screen.findByText(/No delivery policy is configured/),
    ).toBeDefined();
    const deliver = screen.getByRole("button", {
      name: "Deliver candidate",
    }) as HTMLButtonElement;
    const open = screen.getByRole("button", {
      name: "Open pull request",
    }) as HTMLButtonElement;
    expect(deliver.disabled).toBe(true);
    expect(open.disabled).toBe(true);
    expect(
      screen.getByText(/No delivery attempt has been recorded/),
    ).toBeDefined();
  });

  it("sends a selected supported manual base branch", async () => {
    const manualPolicy = {
      ...policy,
      mode: "MANUAL" as const,
      r1Enabled: false,
      manualBaseBranches: ["main", "release"],
    };
    mocks.getCandidateDelivery.mockResolvedValue(
      status({
        riskClass: "R2",
        policy: manualPolicy,
        projection: {
          ...status().projection,
          policy: manualPolicy,
          attempts: [],
          decisions: [],
          checkReceipts: [],
          postMergeChecks: [],
          incidents: [],
          reverts: [],
          issueReceipts: [],
        },
      }),
    );
    mocks.deliverCandidate.mockResolvedValue({
      attempt: attempt({ riskClass: "R2", authorization: "OWNER_MANUAL" }),
      policy: manualPolicy,
      decision: {},
      pullRequest: null,
      checks: null,
      protection: null,
      issueOutcome: null,
      replayed: false,
    });

    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Delivery");
    await user.selectOptions(screen.getByLabelText("Base branch"), "release");
    await user.click(
      screen.getByRole("checkbox", {
        name: "I am the owner and approve this exact merge",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Deliver candidate" }));
    await waitFor(() =>
      expect(mocks.deliverCandidate).toHaveBeenCalledWith(
        "campaign-1",
        "candidate-1",
        {
          operation: "MERGE",
          manual_confirmed: true,
          base_branch: "release",
        },
      ),
    );
  });

  it("blocks R1 salvage auto-merge in the browser", async () => {
    mocks.getCandidateDelivery.mockResolvedValue(
      status({
        riskClass: "R1",
        finalSource: "STAGNATION_SALVAGE",
        salvageRequiresHumanReview: true,
        salvageEvidence: {
          candidateId: "candidate-1",
          commitSha: HEAD,
          attemptId: "attempt-1",
          workItemId: "unit-1",
          frozenChecks: 1,
          passedChecks: 1,
          written: true,
          decision: "DIAGNOSE",
          changedPaths: ["src/lib.rs"],
          checkReceiptIds: ["receipt-1"],
        },
      }),
    );
    renderPanel();
    expect(
      await screen.findByText(/requires authenticated human review/),
    ).toBeDefined();
    const deliver = screen.getByRole("button", {
      name: "Deliver candidate",
    }) as HTMLButtonElement;
    expect(deliver.disabled).toBe(true);
  });

  it("disables R0 delivery when salvage evidence is missing", async () => {
    mocks.getCandidateDelivery.mockResolvedValue(
      status({
        riskClass: "R0",
        finalSource: "STAGNATION_SALVAGE",
        salvageRequiresHumanReview: false,
        salvageEvidence: null,
        salvageError:
          "SALVAGE_EVIDENCE_REQUIRED: stagnation salvage evidence is missing",
      }),
    );
    renderPanel();
    expect(
      (await screen.findAllByText(/SALVAGE_EVIDENCE_REQUIRED/)).length,
    ).toBeGreaterThan(0);
    const deliver = screen.getByRole("button", {
      name: "Deliver candidate",
    }) as HTMLButtonElement;
    expect(deliver.disabled).toBe(true);
  });

  it("renders a typed Core error with retry", async () => {
    mocks.getCandidateDelivery
      .mockRejectedValueOnce(new Error("core unavailable"))
      .mockResolvedValueOnce(status());
    const user = userEvent.setup();
    renderPanel();
    expect(
      await screen.findByText("Core could not read delivery state."),
    ).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Delivery")).toBeDefined();
  });
});
