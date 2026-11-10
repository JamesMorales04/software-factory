import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TraceEventView, WorkflowTraceView } from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { RequestTrace } from "./RequestTrace";

const mocks = vi.hoisted(() => ({
  getRequestTrace: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    readonly code: string | null = null;
    readonly reason: string | null = null;
    readonly body: unknown = null;
    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  },
}));

function event(
  sequence: number,
  kind: string,
  overrides: Partial<TraceEventView> = {},
): TraceEventView {
  return {
    sequence,
    stepId: null,
    kind,
    stateFrom: null,
    stateTo: null,
    payloadSha256: `${sequence}`.padStart(64, "a"),
    recordedAt: "2026-11-09T10:00:00Z",
    payload: { state: "SUBMITTED" },
    ...overrides,
  };
}

function emptyTrace(): WorkflowTraceView {
  return {
    requestId: "request-1",
    run: null,
    trace: null,
    events: [],
    steps: [],
    alerts: [],
    terminal: false,
    eventLimit: 200,
    truncated: false,
    stepLimit: 500,
    stepsTruncated: false,
    replayError: null,
  };
}

function runningTrace(
  overrides: Partial<WorkflowTraceView> = {},
): WorkflowTraceView {
  return {
    requestId: "request-1",
    run: {
      id: "workflow-1",
      requestId: "request-1",
      campaignId: null,
      state: "EXECUTING",
      revision: 5,
      planSha256: "",
      createdAt: "2026-11-09T09:00:00Z",
      updatedAt: "2026-11-09T10:05:00Z",
    },
    trace: {
      events: [
        {
          sequence: 1,
          requestId: "request-1",
          runId: "workflow-1",
          stepId: null,
          kind: "run.created",
          stateFrom: null,
          stateTo: null,
          payloadSha256: "a".repeat(64),
          recordedAt: "2026-11-09T09:00:00Z",
        },
      ],
      finalState: "EXECUTING",
      terminal: false,
      checkpoints: [],
      openSteps: [{ stepId: "step-2", status: "RUNNING", lastSequence: 3 }],
      duplicateSequences: [{ sequence: 2, occurrences: 2 }],
      missingTransitions: [
        {
          sequence: 3,
          expectedFrom: "SUBMITTED",
          observedFrom: "EXECUTING",
        },
      ],
    },
    events: [
      event(1, "run.created", {
        payload: { state: "SUBMITTED" },
      }),
      event(2, "workflow.step.started", {
        stepId: "step-1",
        payload: {
          stepId: "step-1",
          status: "RUNNING",
          state: "raw prompt or token",
          prompt: "raw prompt body",
          token: "super-secret-token",
        },
      }),
      event(3, "workflow.step.started", {
        stepId: "raw prompt or token",
        stateFrom: "SUBMITTED",
        stateTo: "EXECUTING",
        payload: {
          stepId: "step-2",
          status: "RUNNING",
          detail: "[REDACTED]",
        },
      }),
    ],
    steps: [
      {
        stepId: "step-1",
        kind: "unit.execute",
        status: "SUCCEEDED",
        attempt: 1,
        inputSha256: "b".repeat(64),
        outputSha256: "c".repeat(64),
        createdAt: "2026-11-09T09:01:00Z",
        updatedAt: "2026-11-09T09:05:00Z",
        open: false,
        lastSequence: 2,
      },
      {
        stepId: "step-2",
        kind: "unit.verify",
        status: "RUNNING",
        attempt: 1,
        inputSha256: "d".repeat(64),
        outputSha256: "",
        createdAt: "2026-11-09T09:06:00Z",
        updatedAt: "2026-11-09T09:07:00Z",
        open: true,
        lastSequence: 3,
      },
    ],
    alerts: [
      {
        kind: "NON_TERMINAL_STATE",
        severity: "INFO",
        sequence: 3,
        stepId: null,
        occurrences: null,
        expectedFrom: null,
        observedFrom: null,
        expectedState: null,
        observedState: null,
      },
      {
        kind: "DUPLICATE_SEQUENCE",
        severity: "WARNING",
        sequence: 2,
        stepId: null,
        occurrences: 2,
        expectedFrom: null,
        observedFrom: null,
        expectedState: null,
        observedState: null,
      },
      {
        kind: "MISSING_TRANSITION",
        severity: "WARNING",
        sequence: 3,
        stepId: null,
        occurrences: null,
        expectedFrom: "SUBMITTED",
        observedFrom: "EXECUTING",
        expectedState: null,
        observedState: null,
      },
      {
        kind: "OPEN_STEP",
        severity: "INFO",
        sequence: 3,
        stepId: "step-2",
        occurrences: null,
        expectedFrom: null,
        observedFrom: null,
        expectedState: null,
        observedState: null,
      },
      {
        kind: "MALFORMED_EVENT",
        severity: "WARNING",
        sequence: 3,
        stepId: null,
        occurrences: null,
        expectedFrom: null,
        observedFrom: null,
        expectedState: null,
        observedState: null,
      },
    ],
    terminal: false,
    eventLimit: 200,
    truncated: false,
    stepLimit: 500,
    stepsTruncated: false,
    replayError: null,
    ...overrides,
  };
}

function renderTrace(locale?: Locale) {
  const content = <RequestTrace requestId="request-1" />;
  return render(
    locale ? (
      <I18nProvider initialLocale={locale}>{content}</I18nProvider>
    ) : (
      content
    ),
  );
}

describe("RequestTrace", () => {
  beforeEach(() => {
    mocks.getRequestTrace.mockReset().mockResolvedValue(emptyTrace());
  });

  it("shows a loading state while Core reads the trace", () => {
    mocks.getRequestTrace.mockReturnValue(new Promise(() => undefined));
    renderTrace();
    expect(screen.getByText("Loading workflow trace…")).toBeDefined();
  });

  it("shows an empty state when the request has no workflow run", async () => {
    renderTrace();
    expect(await screen.findByText("No workflow run yet")).toBeDefined();
    expect(
      screen.getByText(/A trace appears after Core creates/),
    ).toBeDefined();
  });

  it("shows a Core error and retries the authenticated read", async () => {
    const user = userEvent.setup();
    mocks.getRequestTrace
      .mockRejectedValueOnce(new Error("Core trace unavailable"))
      .mockResolvedValueOnce(emptyTrace());
    renderTrace();

    expect(await screen.findByText("Core trace unavailable")).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Retry trace" }));
    expect(await screen.findByText("No workflow run yet")).toBeDefined();
    expect(mocks.getRequestTrace).toHaveBeenCalledTimes(2);
  });

  it("renders replay alerts, per-step status and the redacted event log", async () => {
    mocks.getRequestTrace.mockResolvedValue(runningTrace());
    renderTrace();

    expect(await screen.findByText("Workflow trace")).toBeDefined();
    expect(screen.getByText("Duplicate sequence")).toBeDefined();
    expect(screen.getByText("Missing transition")).toBeDefined();
    expect(screen.getByText("Open step")).toBeDefined();
    expect(screen.getByText("Malformed event")).toBeDefined();
    expect(screen.getAllByText("In progress").length).toBeGreaterThan(0);
    expect(screen.getByText("unit.execute")).toBeDefined();
    expect(screen.getByText("unit.verify")).toBeDefined();
    expect(screen.getByText("Open in replay")).toBeDefined();
    expect(screen.getAllByText("workflow.step.started").length).toBeGreaterThan(
      0,
    );
    expect(screen.getByText("SUBMITTED → EXECUTING")).toBeDefined();
    expect(screen.getAllByText("[REDACTED]").length).toBeGreaterThan(1);

    // Negative redaction check: neither the raw prompt nor the raw token can
    // be rendered even if a malicious/buggy payload tries to include one.
    expect(screen.queryByText("raw prompt body")).toBeNull();
    expect(screen.queryByText("super-secret-token")).toBeNull();
    expect(screen.queryByText("raw prompt or token")).toBeNull();
  });

  it("renders a rejected replay as unknown rather than in progress", async () => {
    const base = runningTrace();
    mocks.getRequestTrace.mockResolvedValue({
      ...base,
      run: base.run ? { ...base.run, state: "ACCEPTED" } : null,
      trace: null,
      terminal: false,
      replayError: "ambiguous ledger",
      alerts: [
        {
          kind: "REPLAY_REJECTED",
          severity: "CRITICAL",
          sequence: 3,
          stepId: null,
          occurrences: null,
          expectedFrom: null,
          observedFrom: null,
          expectedState: null,
          observedState: null,
        },
      ],
    });
    renderTrace();

    expect(await screen.findByText("Unknown — replay rejected")).toBeDefined();
    expect(screen.queryByText("In progress")).toBeNull();
  });

  it("reports bounded step history", async () => {
    const base = runningTrace();
    mocks.getRequestTrace.mockResolvedValue(
      runningTrace({
        steps: base.steps,
        stepsTruncated: true,
        stepLimit: 2,
        alerts: [
          ...base.alerts,
          {
            kind: "STEP_TRUNCATED",
            severity: "WARNING",
            sequence: null,
            stepId: null,
            occurrences: null,
            expectedFrom: null,
            observedFrom: null,
            expectedState: null,
            observedState: null,
          },
        ],
      }),
    );
    renderTrace();

    expect(await screen.findByText(/Showing the newest 2 steps/)).toBeDefined();
    expect(screen.getByText("2 step(s) · limit 2")).toBeDefined();
    expect(screen.getByText("Step trace truncated")).toBeDefined();
  });

  it("renders a bounded long event list and reports truncation", async () => {
    const events = Array.from({ length: 150 }, (_, index) =>
      event(index + 1, "workflow.step.started", {
        stepId: "step-long",
        payload: { stepId: "step-long", status: "RUNNING" },
      }),
    );
    mocks.getRequestTrace.mockResolvedValue(
      runningTrace({
        events,
        truncated: true,
        eventLimit: 150,
      }),
    );
    renderTrace();

    expect(await screen.findByText("#150")).toBeDefined();
    expect(screen.getByText(/Showing the newest 150 events/)).toBeDefined();
    expect(screen.getByLabelText("Workflow event log")).toBeDefined();
  });

  it("renders the trace surface in Spanish", async () => {
    mocks.getRequestTrace.mockResolvedValue(runningTrace());
    renderTrace("es");

    expect(
      await screen.findByRole("heading", {
        name: "Traza del flujo de trabajo",
      }),
    ).toBeDefined();
    expect(screen.getByText("Registro de eventos")).toBeDefined();
    expect(screen.getByText("Secuencia duplicada")).toBeDefined();
  });
});
