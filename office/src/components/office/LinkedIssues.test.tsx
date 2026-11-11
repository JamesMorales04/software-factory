import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { IssueLinkProjection } from "../../coreApi";
import { CoreApiError } from "../../coreApi";
import { I18nProvider } from "../../i18n";
import { LinkedIssues } from "./LinkedIssues";

const mocks = vi.hoisted(() => ({
  retryIssueDelivery: vi.fn(),
}));

vi.mock("../../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    constructor(
      readonly status: number,
      message: string,
      readonly body?: unknown,
    ) {
      super(message);
    }
  },
}));

const link: IssueLinkProjection = {
  id: "link-7",
  projectId: "project-1",
  requestId: "request-1",
  campaignId: "campaign-1",
  candidateId: "candidate-1",
  repositoryOwner: "acme",
  repositoryName: "widgets",
  repositoryFullName: "acme/widgets",
  issueNumber: 7,
  githubIssueId: 70,
  issueTitle: "Fix the widget",
  role: "PRIMARY",
  state: "IN_REVIEW",
  deliveryState: "COMMENTED",
  resolution: "UNRESOLVED",
  workUnitId: "unit-a",
  pullRequestNumber: 42,
  pullRequestUrl: "https://github.com/acme/widgets/pull/42",
  mergeSha: null,
  mergedAt: null,
  createdAt: "2026-11-08T00:00:00Z",
  updatedAt: "2026-11-08T00:00:00Z",
  latestError: null,
  events: [
    {
      id: "event-1",
      issueLinkId: "link-7",
      campaignId: "campaign-1",
      candidateId: "candidate-1",
      action: "COMMENT",
      eventKey: "comment:opened:candidate-1:42",
      marker: "<!-- factory:issue-link:link-7:OPENED:candidate-1:42 -->",
      body: "comment",
      labelName: null,
      status: "APPLIED",
      attempts: 1,
      externalId: "9001",
      errorKind: null,
      errorDetail: null,
      lastAttemptAt: "2026-11-08T00:00:00Z",
      appliedAt: "2026-11-08T00:00:00Z",
      createdAt: "2026-11-08T00:00:00Z",
      updatedAt: "2026-11-08T00:00:00Z",
    },
  ],
};

function renderLinked(links: IssueLinkProjection[], onRetried = vi.fn()) {
  render(
    <I18nProvider>
      <LinkedIssues links={links} onRetried={onRetried} />
    </I18nProvider>,
  );
  return onRetried;
}

describe("LinkedIssues", () => {
  beforeEach(() => {
    mocks.retryIssueDelivery.mockReset().mockResolvedValue({});
  });

  it("renders Core-backed linked issue state, PR, merge and receipts", () => {
    renderLinked([
      {
        ...link,
        state: "DONE",
        deliveryState: "CLOSED",
        mergeSha: "e".repeat(40),
        mergedAt: "2026-11-08T01:00:00Z",
        events: [
          ...link.events,
          {
            ...link.events[0],
            id: "event-2",
            action: "CLOSE",
            eventKey: "close:merge:eeee",
            marker: null,
            body: null,
            status: "APPLIED",
            externalId: "7",
          },
        ],
      },
    ]);
    expect(screen.getByText("acme/widgets#7")).toBeTruthy();
    expect(screen.getByText("Fix the widget")).toBeTruthy();
    expect(screen.getByText("DONE")).toBeTruthy();
    expect(screen.getByText("Delivery: CLOSED")).toBeTruthy();
    expect(screen.getByRole("link", { name: "#42" }).getAttribute("href")).toBe(
      "https://github.com/acme/widgets/pull/42",
    );
    expect(screen.getByText(/Merge/)).toBeTruthy();
    expect(screen.getByText(/Delivery receipts \(2\)/)).toBeTruthy();
  });

  it("retries a typed failed delivery event and refreshes from Core", async () => {
    const onRetried = vi.fn();
    renderLinked(
      [
        {
          ...link,
          state: "DONE",
          deliveryState: "FAILED",
          mergeSha: "e".repeat(40),
          latestError: {
            kind: "GITHUB_UPSTREAM",
            detail: "GitHub is unreachable",
            retryable: true,
          },
          events: [
            {
              ...link.events[0],
              id: "event-close",
              action: "CLOSE",
              eventKey: "close:merge:eeee",
              marker: null,
              body: null,
              status: "FAILED",
              errorKind: "GITHUB_UPSTREAM",
              errorDetail: "GitHub is unreachable",
            },
          ],
        },
      ],
      onRetried,
    );
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(mocks.retryIssueDelivery).toHaveBeenCalledWith(
      "link-7",
      "event-close",
    );
    await waitFor(() => expect(onRetried).toHaveBeenCalledTimes(1));
  });

  it("shows a typed retry error without inventing delivery state", async () => {
    mocks.retryIssueDelivery.mockRejectedValue(
      new CoreApiError(403, "GitHub denied the request"),
    );
    renderLinked([
      {
        ...link,
        deliveryState: "BLOCKED",
        latestError: {
          kind: "GITHUB_FORBIDDEN",
          detail: "GitHub denied the request",
          retryable: false,
        },
        events: [
          {
            ...link.events[0],
            id: "event-blocked",
            action: "COMMENT",
            status: "BLOCKED",
            errorKind: "GITHUB_FORBIDDEN",
            errorDetail: "GitHub denied the request",
          },
        ],
      },
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("GitHub denied the request")).toBeTruthy();
  });

  it("states that a request without issue keys has no mapping", () => {
    renderLinked([]);
    expect(screen.getByText(/did not select a GitHub issue/)).toBeTruthy();
  });
});
