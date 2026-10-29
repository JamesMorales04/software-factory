import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ParticipantPage,
  ParticipantRecord,
  ProviderToolEventProjection,
} from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { AgentOffice } from "./AgentOffice";

const mocks = vi.hoisted(() => ({
  listParticipants: vi.fn(),
  listCampaigns: vi.fn(),
  listProjects: vi.fn(),
  listToolEvents: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status = 500;
  },
}));

const baseSession = {
  attempt_id: "attempt-1",
  worker_id: "worker-1",
  generation: 2,
  provider: "opencode",
  model: "deepseek-v4.1-flash",
  provider_mode: "build",
  provider_session_id: "ses-external-1",
  input_tokens: 100,
  output_tokens: 40,
  total_tokens: 140,
  cost_micros: 25,
  usage_known: true,
  created_at: "2026-10-27T10:00:00Z",
  updated_at: "2026-10-27T10:05:00Z",
};

const implementer: ParticipantRecord = {
  participant_id: "implementer:session-1",
  role: "IMPLEMENTER",
  identity_kind: "PROVIDER_SESSION",
  identity_id: "session-1",
  principal_id: "implementer-1",
  project_id: "project-1",
  action_id: "action-1",
  campaign_id: "campaign-1",
  request_id: null,
  work_unit_id: "unit-a",
  run_id: "run-1",
  attempt_id: "attempt-1",
  attempt_generation: 2,
  worker_id: "worker-1",
  requested_route: {
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    mode: "build",
  },
  observed_route: null,
  route_provenance: "PROVIDER_SESSION_BINDING_REQUESTED",
  session: {
    session_record_id: "session-1",
    state: "ACTIVE",
    cleanup: "PENDING",
    cleanup_receipt: null,
    error: null,
    ...baseSession,
  },
  status: "ACTIVE",
  status_source: "PROVIDER_SESSION",
  lease_state: "LIVE",
  observation: "CURRENT",
  failure_reason: null,
  usage: {
    input_tokens: 100,
    output_tokens: 40,
    total_tokens: 140,
    cost_micros: 25,
    usage_known: true,
  },
  usage_provenance: "PROVIDER_SESSION_BINDING",
  evidence: {
    candidate_id: "candidate-1",
    candidate_commit: "b".repeat(40),
    candidate_tree: "c".repeat(40),
    gate_outcome: "ACCEPTED",
    gate_decided_at: "2026-10-27T10:09:00Z",
    verifier_receipt_id: "verifier-1",
    reviewer_receipt_id: "reviewer-1",
  },
  model_call_count: null,
  model_call_state: null,
  observation_note: "Core holds a live attempt lease.",
  recorded_at: "2026-10-27T10:00:00Z",
  last_observed_at: "2026-10-27T10:05:00Z",
};

const staleSession = {
  session_record_id: "session-2",
  state: "ACTIVE",
  cleanup: "MISSING",
  cleanup_receipt: null,
  error: "provider cleanup unconfirmed",
  ...baseSession,
  updated_at: "2026-10-27T10:01:00Z",
};

const staleImplementer: ParticipantRecord = {
  ...implementer,
  participant_id: "implementer:session-2",
  identity_id: "session-2",
  attempt_id: "attempt-2",
  attempt_generation: 1,
  work_unit_id: "unit-b",
  run_id: "run-2",
  status: "ACTIVE",
  lease_state: "EXPIRED",
  observation: "STALE",
  evidence: null,
  failure_reason: "provider cleanup unconfirmed",
  session: staleSession,
  last_observed_at: "2026-10-27T10:01:00Z",
};

const lead: ParticipantRecord = {
  participant_id: "lead:lead-1",
  role: "LEAD",
  identity_kind: "LEAD_REQUEST",
  identity_id: "lead-1",
  principal_id: "owner",
  project_id: "project-1",
  action_id: "action-1",
  campaign_id: "campaign-1",
  request_id: "lead-1",
  work_unit_id: null,
  run_id: null,
  attempt_id: null,
  attempt_generation: null,
  worker_id: null,
  requested_route: {
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    mode: null,
  },
  observed_route: null,
  route_provenance: "LEAD_MODEL_CALL_RESERVATION",
  session: null,
  status: "READY",
  status_source: "LEAD_REQUEST",
  lease_state: "NOT_APPLICABLE",
  observation: "CURRENT",
  failure_reason: null,
  usage: {
    input_tokens: 10,
    output_tokens: 5,
    total_tokens: 15,
    cost_micros: 3,
    usage_known: true,
  },
  usage_provenance: "LEAD_MODEL_CALLS",
  evidence: null,
  model_call_count: 1,
  model_call_state: "SETTLED",
  observation_note: "A Lead request is a planning operation.",
  recorded_at: "2026-10-27T09:30:00Z",
  last_observed_at: "2026-10-27T09:32:00Z",
};

const page: ParticipantPage = {
  participants: [implementer, staleImplementer, lead],
  page: {
    limit: 200,
    offset: 0,
    returned: 3,
    has_more: false,
    order: "last_observed_at DESC, participant_id ASC",
    as_of: "2026-10-27T10:10:00Z",
  },
  sources: [
    {
      role: "IMPLEMENTER",
      source: "provider_sessions -> attempts -> runs -> campaigns",
      note: "One participant per observed provider session generation.",
    },
  ],
};

const toolEvents: ProviderToolEventProjection = {
  session: {
    participant_id: "implementer:session-1",
    session_record_id: "session-1",
    attempt_id: "attempt-1",
    run_id: "run-1",
    campaign_id: "campaign-1",
    worker_id: "worker-1",
    generation: 2,
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    provider_mode: "build",
    state: "ACTIVE",
    lease_state: "LIVE",
    cleanup: "PENDING",
    updated_at: "2026-10-27T10:05:00Z",
  },
  events: [
    {
      sequence: 1,
      call_ref: "abc123",
      tool: "write_file",
      tool_digest: "digest",
      decision: "REQUEST",
      detail: "tool requested",
      detail_state: "RECORDED",
      recorded_at: "2026-10-27T10:04:00Z",
      provenance: "provider_tool_events",
    },
    {
      sequence: 2,
      call_ref: "abc123",
      tool: "final",
      tool_digest: "digest2",
      decision: "RESULT",
      detail: null,
      detail_state: "WITHHELD",
      recorded_at: "2026-10-27T10:05:00Z",
      provenance: "provider_tool_events",
    },
  ],
  after_sequence: 0,
  limit: 100,
  returned: 2,
  next_sequence: 2,
  has_more: false,
  discarded: 0,
  source: "provider_tool_events",
  note: "Bounded projection.",
};

function LocationProbe() {
  const location = useLocation();
  return (
    <output data-testid="location">
      {location.pathname}
      {location.search}
    </output>
  );
}

function renderOffice(path = "/agents", locale?: Locale) {
  const content = (
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="/agents"
          element={
            <>
              <AgentOffice />
              <LocationProbe />
            </>
          }
        />
        <Route
          path="/agents/:participantId"
          element={
            <>
              <AgentOffice />
              <LocationProbe />
            </>
          }
        />
      </Routes>
    </MemoryRouter>
  );
  return render(
    locale ? (
      <I18nProvider initialLocale={locale}>{content}</I18nProvider>
    ) : (
      content
    ),
  );
}

describe("AgentOffice", () => {
  beforeEach(() => {
    mocks.listParticipants.mockReset().mockResolvedValue(page);
    mocks.listCampaigns.mockReset().mockResolvedValue([
      {
        id: "campaign-1",
        action_id: "action-1",
        status: "AUTHORIZED",
        created_at: "2026-10-27T09:00:00Z",
      },
    ]);
    mocks.listProjects.mockReset().mockResolvedValue([
      {
        id: "project-1",
        repository: "/tmp/project",
        base_commit: "a".repeat(40),
        created_at: "2026-10-27T08:00:00Z",
      },
    ]);
    mocks.listToolEvents.mockReset().mockResolvedValue(toolEvents);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders truthful zones, stale ambiguity and participant cards", async () => {
    renderOffice();
    await screen.findByRole("heading", { name: "Agent Office" });
    expect(screen.getByText("Implementation")).toBeDefined();
    expect(screen.getByText("Planning")).toBeDefined();
    expect(screen.getByText("Operations")).toBeDefined();
    expect(
      screen.getByText(/unresolved participant observation is stale/),
    ).toBeDefined();
    expect(
      screen.getByRole("button", {
        name: /Inspect Implementer implementer:session-1/,
      }),
    ).toBeDefined();
    expect(
      screen.getByRole("button", { name: /Inspect Lead lead:lead-1/ }),
    ).toBeDefined();
    expect(screen.getByRole("heading", { name: "Verification" })).toBeDefined();
    expect(screen.getByRole("heading", { name: "Review" })).toBeDefined();
    expect(screen.getByTestId("location").textContent).toBe("/agents");
  });

  it("renders zones, filters and participant controls in Spanish", async () => {
    renderOffice("/agents", "es");
    await screen.findByRole("heading", { name: "Oficina de agentes" });
    expect(screen.getByText("Planificación")).toBeDefined();
    expect(screen.getByText("Implementación")).toBeDefined();
    expect(screen.getByText("Verificación")).toBeDefined();
    expect(screen.getByText("Revisión")).toBeDefined();
    expect(screen.getByText("Operaciones")).toBeDefined();
    expect(screen.getByLabelText("Rol")).toBeDefined();
    expect(screen.getByLabelText("Proveedor")).toBeDefined();
    expect(
      screen.getByRole("button", {
        name: /Inspeccionar Implementador implementer:session-1/,
      }),
    ).toBeDefined();
    expect(screen.getByRole("heading", { name: "Verificación" })).toBeDefined();
    expect(screen.getByText("Estado veraz")).toBeDefined();
  });

  it("selects a participant, shows evidence and recorded tool events", async () => {
    renderOffice();
    await screen.findByRole("heading", { name: "Agent Office" });
    await userEvent.click(
      screen.getByRole("button", {
        name: /Inspect Implementer implementer:session-1/,
      }),
    );
    await screen.findByTestId("participant-inspector");
    expect(
      decodeURIComponent(screen.getByTestId("location").textContent ?? ""),
    ).toContain("/agents/implementer:session-1");
    expect(screen.getByText("Quality evidence")).toBeDefined();
    expect(screen.getByText("campaign-1")).toBeDefined();
    expect(screen.getByText("ACCEPTED")).toBeDefined();
    const events = await screen.findByTestId("tool-event-table");
    expect(events.textContent).toContain("write_file");
    expect(events.textContent).toContain("tool requested");
    expect(events.textContent).toContain("provider content withheld");
    expect(mocks.listToolEvents).toHaveBeenCalledWith("session-1", 0, 100);
  });

  it("collapses and restores the visual-view inspector", async () => {
    renderOffice();
    await screen.findByRole("heading", { name: "Agent Office" });
    expect(screen.getByText("Select a participant")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Hide inspector" }),
    );
    expect(screen.queryByText("Select a participant")).toBeNull();
    await userEvent.click(
      screen.getByRole("button", { name: "Show inspector" }),
    );
    expect(screen.getByText("Select a participant")).toBeDefined();
  });

  it("keeps selection across floor, table and timeline views", async () => {
    renderOffice("/agents/implementer%3Asession-1");
    await screen.findByTestId("participant-inspector");
    await userEvent.click(screen.getByRole("tab", { name: /Table/ }));
    expect(screen.getByTestId("agent-table")).toBeDefined();
    expect(screen.getByTestId("participant-inspector")).toBeDefined();
    await userEvent.click(screen.getByRole("tab", { name: /Timeline/ }));
    expect(screen.getByTestId("agent-timeline")).toBeDefined();
    expect(screen.getByTestId("participant-inspector")).toBeDefined();
    expect(screen.getByTestId("location").textContent).toContain(
      "view=timeline",
    );
  });

  it("applies server-backed filters and reconstructs them from the URL", async () => {
    renderOffice("/agents?role=IMPLEMENTER&campaign=campaign-1");
    await screen.findByRole("heading", { name: "Agent Office" });
    await waitFor(() =>
      expect(mocks.listParticipants).toHaveBeenCalledWith(
        expect.objectContaining({
          role: "IMPLEMENTER",
          campaignId: "campaign-1",
          limit: 200,
        }),
      ),
    );
    expect(screen.getByLabelText("Role")).toHaveProperty(
      "value",
      "IMPLEMENTER",
    );
    expect(screen.getByText(/Projection scope only/)).toBeDefined();
  });

  it("renders only participants returned for the active project and campaign scope", async () => {
    mocks.listParticipants.mockImplementation(
      (filters: { projectId?: string; campaignId?: string }) =>
        Promise.resolve(
          filters.projectId === "project-1" &&
            filters.campaignId === "campaign-1"
            ? {
                ...page,
                participants: [implementer],
                page: { ...page.page, returned: 1 },
              }
            : page,
        ),
    );
    renderOffice("/agents?project=project-1&campaign=campaign-1");
    await screen.findByRole("heading", { name: "Agent Office" });
    await waitFor(() =>
      expect(mocks.listParticipants).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          campaignId: "campaign-1",
        }),
      ),
    );
    expect(
      screen.getByTestId(`office-avatar-${implementer.participant_id}`),
    ).toBeDefined();
    expect(
      screen.queryByTestId(`office-avatar-${lead.participant_id}`),
    ).toBeNull();
    expect(screen.getAllByRole("button", { name: /^Inspect / })).toHaveLength(
      1,
    );
  });

  it("preserves the last snapshot and surfaces a disconnected refresh", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mocks.listParticipants
      .mockReset()
      .mockResolvedValueOnce(page)
      .mockRejectedValue(new Error("Core is unreachable"));
    renderOffice();
    await screen.findByRole("heading", { name: "Agent Office" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5100);
    });
    expect(screen.getByText(/Core refresh failed/)).toBeDefined();
    expect(
      screen.getByText(/Showing the last authoritative snapshot/),
    ).toBeDefined();
    expect(screen.getByText("Implementation")).toBeDefined();
  });

  it("shows empty and unknown participant states without inventing activity", async () => {
    mocks.listParticipants.mockResolvedValue({
      ...page,
      participants: [],
      page: { ...page.page, returned: 0 },
    });
    renderOffice();
    await screen.findByRole("heading", { name: "Agent Office" });
    expect(screen.getByText("No recorded participants")).toBeDefined();
    expect(screen.queryByTestId("agent-floor")).toBeNull();
  });

  it("does not inject a deep-linked participant outside the active filters", async () => {
    mocks.listParticipants.mockImplementation(
      (filters: { participantId?: string; campaignId?: string }) => {
        if (filters.participantId) {
          return Promise.resolve({
            ...page,
            participants: [],
            page: { ...page.page, returned: 0 },
          });
        }
        return Promise.resolve({
          ...page,
          participants: [],
          page: { ...page.page, returned: 0 },
        });
      },
    );
    renderOffice(
      "/agents/implementer%3Asession-1?campaign=campaign-2&role=IMPLEMENTER",
    );
    await screen.findByRole("heading", { name: "Agent Office" });
    expect(screen.getByText("Participant not found")).toBeDefined();
    await waitFor(() =>
      expect(mocks.listParticipants).toHaveBeenCalledWith(
        expect.objectContaining({
          participantId: "implementer:session-1",
          campaignId: "campaign-2",
          role: "IMPLEMENTER",
          limit: 1,
        }),
      ),
    );
    expect(screen.queryByTestId("participant-inspector")).toBeNull();
  });
});
