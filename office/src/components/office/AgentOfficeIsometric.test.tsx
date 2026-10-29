import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { I18nProvider } from "../../i18n";
import { AgentOfficeIsometric } from "./AgentOfficeIsometric";

const ROLES: ParticipantRole[] = [
  "LEAD",
  "IMPLEMENTER",
  "VERIFIER",
  "REVIEWER",
  "WORKER",
];

function participant(
  index: number,
  role: ParticipantRole,
  extra: Partial<ParticipantRecord> = {},
): ParticipantRecord {
  const id = `participant-${String(index).padStart(2, "0")}`;
  return {
    participant_id: `${role.toLowerCase()}:${id}`,
    role,
    identity_kind: role === "WORKER" ? "WORKER" : "PROVIDER_SESSION",
    identity_id: id,
    principal_id: null,
    project_id: "project-1",
    action_id: null,
    campaign_id: "campaign-1",
    request_id: null,
    work_unit_id: null,
    run_id: null,
    attempt_id: null,
    attempt_generation: null,
    worker_id: null,
    requested_route: null,
    observed_route: null,
    route_provenance: "NOT_EXPOSED",
    session: null,
    status: "ACTIVE",
    status_source: "PROVIDER_SESSION",
    lease_state: "LIVE",
    observation: "CURRENT",
    failure_reason: null,
    usage: null,
    usage_provenance: "NOT_EXPOSED",
    evidence: null,
    model_call_count: null,
    model_call_state: null,
    observation_note: "",
    recorded_at: "2026-10-28T10:00:00Z",
    last_observed_at: "2026-10-28T10:05:00Z",
    ...extra,
  };
}

function mixedParticipants(count: number): ParticipantRecord[] {
  return Array.from({ length: count }, (_, index) =>
    participant(index, ROLES[index % ROLES.length]),
  );
}

function renderFloor(
  participants: ParticipantRecord[],
  selectedId: string | null = null,
  onSelect: (participantId: string) => void = () => {},
  locale: "en" | "es" = "en",
) {
  return render(
    <I18nProvider initialLocale={locale}>
      <AgentOfficeIsometric
        participants={participants}
        selectedId={selectedId}
        onSelect={onSelect}
      />
    </I18nProvider>,
  );
}

function avatarButtons() {
  return document.querySelectorAll<HTMLButtonElement>("[data-office-avatar]");
}

function slotFingerprint(participantId: string): string {
  const avatar = screen.getByTestId(`office-avatar-${participantId}`);
  return [
    avatar.getAttribute("data-slot-index"),
    avatar.getAttribute("data-column"),
    avatar.getAttribute("data-row"),
    avatar.getAttribute("data-anchor-x"),
    avatar.getAttribute("data-anchor-y"),
  ].join(":");
}

describe("AgentOfficeIsometric", () => {
  it("shows an honest empty state and renders no fabricated avatars", () => {
    renderFloor([]);
    expect(avatarButtons()).toHaveLength(0);
    expect(screen.getByText("No recorded participants")).toBeDefined();
    expect(screen.queryByTestId("agent-floor")).toBeNull();
  });

  it.each([1, 5, 12, 30])(
    "renders one avatar and one station slot for each of %i entities",
    (count) => {
      const participants = mixedParticipants(count);
      renderFloor(participants);
      const avatars = avatarButtons();
      expect(avatars).toHaveLength(count);
      expect(
        new Set([...avatars].map((avatar) => avatar.dataset.participantId))
          .size,
      ).toBe(count);
      for (const record of participants) {
        expect(
          screen.getByTestId(`office-avatar-${record.participant_id}`),
        ).toBeDefined();
        expect(
          screen.getByRole("button", {
            name: new RegExp(
              `Inspect ${record.role.charAt(0)}${record.role
                .slice(1)
                .toLowerCase()} ${record.participant_id}`,
            ),
          }),
        ).toBeDefined();
      }
      expect(screen.getByTestId("agent-floor")).toBeDefined();
    },
  );

  it("renders 30 implementers in the implementation station only", () => {
    const participants = Array.from({ length: 30 }, (_, index) =>
      participant(index, "IMPLEMENTER"),
    );
    renderFloor(participants);
    expect(avatarButtons()).toHaveLength(30);
    const implementation = screen.getByRole("region", {
      name: "Implementation",
    });
    expect(
      implementation.querySelectorAll("[data-office-avatar]"),
    ).toHaveLength(30);
    expect(screen.getByText("30 recorded")).toBeDefined();
    const looks = new Set(
      [...implementation.querySelectorAll("[data-office-avatar]")].map(
        (avatar) => (avatar as HTMLElement).dataset.look,
      ),
    );
    expect(looks.size).toBe(30);
  });

  it("composes one studio shell with one room per role station", () => {
    renderFloor(mixedParticipants(5));
    expect(document.querySelectorAll(".office-studio-shell")).toHaveLength(1);
    expect(document.querySelectorAll(".office-room")).toHaveLength(5);
    expect(document.querySelectorAll(".office-zone")).toHaveLength(5);
    expect(
      document.querySelectorAll(".office-studio-shell .office-hallway").length,
    ).toBeGreaterThan(0);
    expect(document.querySelectorAll(".office-avatar-figure")).toHaveLength(5);
  });

  it("renders five active implementers as five distinct characters", () => {
    const participants = Array.from({ length: 5 }, (_, index) =>
      participant(index, "IMPLEMENTER", {
        observation: "CURRENT",
        status: "ACTIVE",
        lease_state: "LIVE",
      }),
    );
    renderFloor(participants);
    const avatars = [...avatarButtons()];
    expect(avatars).toHaveLength(5);
    expect(new Set(avatars.map((avatar) => avatar.dataset.look)).size).toBe(5);
    for (const avatar of avatars) {
      expect(avatar.getAttribute("data-working")).toBe("true");
      expect(avatar.querySelector(".office-avatar-monitor")).not.toBeNull();
    }
  });

  it("does not present final or stale participants as workstations", () => {
    const final = participant(0, "IMPLEMENTER", {
      observation: "FINAL",
      status: "COMPLETED",
      lease_state: "RELEASED",
    });
    const stale = participant(1, "IMPLEMENTER", {
      observation: "STALE",
      status: "ACTIVE",
      lease_state: "EXPIRED",
    });
    renderFloor([final, stale]);
    for (const record of [final, stale]) {
      const avatar = screen.getByTestId(
        `office-avatar-${record.participant_id}`,
      );
      expect(avatar.getAttribute("data-working")).toBe("false");
      expect(avatar.querySelector(".office-avatar-monitor")).toBeNull();
      expect(avatar.querySelector(".office-avatar-idle-ring")).not.toBeNull();
    }
  });

  it("keeps identity-stable station slots across re-renders and input order", () => {
    const participants = mixedParticipants(12);
    const view = renderFloor(participants);
    const slots = new Map(
      participants.map((record) => [
        record.participant_id,
        slotFingerprint(record.participant_id),
      ]),
    );
    view.rerender(
      <I18nProvider initialLocale="en">
        <AgentOfficeIsometric
          participants={[...participants].reverse()}
          selectedId={null}
          onSelect={() => {}}
        />
      </I18nProvider>,
    );
    for (const record of participants) {
      expect(slotFingerprint(record.participant_id)).toBe(
        slots.get(record.participant_id),
      );
    }
  });

  it("wires pointer and keyboard selection to the participant identity", async () => {
    const user = userEvent.setup();
    const participants = mixedParticipants(5);
    const onSelect = vi.fn();
    const view = renderFloor(participants, null, onSelect);
    const target = participants[1];
    const avatar = screen.getByTestId(`office-avatar-${target.participant_id}`);

    await user.click(avatar);
    expect(onSelect).toHaveBeenLastCalledWith(target.participant_id);

    avatar.focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenLastCalledWith(target.participant_id);
    expect(onSelect).toHaveBeenCalledTimes(2);

    view.rerender(
      <I18nProvider initialLocale="en">
        <AgentOfficeIsometric
          participants={participants}
          selectedId={target.participant_id}
          onSelect={onSelect}
        />
      </I18nProvider>,
    );
    expect(
      screen
        .getByTestId(`office-avatar-${target.participant_id}`)
        .getAttribute("aria-pressed"),
    ).toBe("true");
  });

  it("moves focus between avatars with the arrow keys", async () => {
    const user = userEvent.setup();
    const participants = mixedParticipants(5);
    renderFloor(participants);
    const first = screen.getByTestId(
      `office-avatar-${participants[0].participant_id}`,
    );
    first.focus();
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).not.toBe(first);
    expect(
      (document.activeElement as HTMLElement).hasAttribute(
        "data-office-avatar",
      ),
    ).toBe(true);
  });

  it("marks stale and unknown observations without inventing activity", () => {
    const stale = participant(0, "IMPLEMENTER", {
      status: "ACTIVE",
      observation: "STALE",
      lease_state: "EXPIRED",
    });
    const unknown = participant(1, "IMPLEMENTER", {
      status: "MYSTERY",
      observation: "UNKNOWN",
      lease_state: "NOT_APPLICABLE",
    });
    renderFloor([stale, unknown]);
    const staleAvatar = screen.getByTestId(
      `office-avatar-${stale.participant_id}`,
    );
    expect(staleAvatar.getAttribute("data-observation")).toBe("STALE");
    expect(staleAvatar.getAttribute("aria-label")).toContain("STALE");
    const unknownAvatar = screen.getByTestId(
      `office-avatar-${unknown.participant_id}`,
    );
    expect(unknownAvatar.getAttribute("data-observation")).toBe("UNKNOWN");
    expect(unknownAvatar.getAttribute("aria-label")).toContain("UNKNOWN");
    expect(avatarButtons()).toHaveLength(2);
  });

  it("renders one avatar for duplicate participant identities", () => {
    const first = participant(0, "IMPLEMENTER");
    const duplicate = {
      ...participant(1, "IMPLEMENTER"),
      participant_id: first.participant_id,
    };
    renderFloor([first, duplicate]);
    expect(avatarButtons()).toHaveLength(1);
    expect(
      screen.getByText(/duplicate participant identity was ignored/),
    ).toBeDefined();
  });

  it("clamps zoom in and out and restores the fitted camera", async () => {
    const user = userEvent.setup();
    renderFloor(mixedParticipants(5));
    const initial = screen.getByTestId("office-zoom").getAttribute("data-zoom");
    const zoomIn = screen.getByRole("button", { name: "Zoom in" });
    const zoomOut = screen.getByRole("button", { name: "Zoom out" });

    for (
      let step = 0;
      step < 20 && !zoomIn.hasAttribute("disabled");
      step += 1
    ) {
      await user.click(zoomIn);
    }
    expect(screen.getByTestId("office-zoom").getAttribute("data-zoom")).toBe(
      "2.000",
    );
    expect(zoomIn.hasAttribute("disabled")).toBe(true);

    for (
      let step = 0;
      step < 30 && !zoomOut.hasAttribute("disabled");
      step += 1
    ) {
      await user.click(zoomOut);
    }
    expect(screen.getByTestId("office-zoom").getAttribute("data-zoom")).toBe(
      "0.450",
    );
    expect(zoomOut.hasAttribute("disabled")).toBe(true);

    await user.click(screen.getByRole("button", { name: "Fit scene" }));
    expect(screen.getByTestId("office-zoom").getAttribute("data-zoom")).toBe(
      initial,
    );
  });

  it("clamps panning at the scene edge", async () => {
    const user = userEvent.setup();
    renderFloor(mixedParticipants(5));
    const zoomIn = screen.getByRole("button", { name: "Zoom in" });
    for (
      let step = 0;
      step < 20 && !zoomIn.hasAttribute("disabled");
      step += 1
    ) {
      await user.click(zoomIn);
    }
    const panLeft = screen.getByRole("button", { name: "Pan left" });
    const world = screen.getByTestId("office-scene-world");
    for (
      let step = 0;
      step < 40 && !panLeft.hasAttribute("disabled");
      step += 1
    ) {
      await user.click(panLeft);
    }
    expect(panLeft.hasAttribute("disabled")).toBe(true);
    const clamped = world.style.transform;
    expect(clamped).toContain("translate(");
    expect(clamped).not.toContain("NaN");
  });

  it("localizes the floor, controls and station headings", () => {
    renderFloor(mixedParticipants(5), null, () => {}, "es");
    expect(
      screen.getByRole("region", { name: "Planta isométrica de la Oficina" }),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "Ampliar" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Reducir" })).toBeDefined();
    expect(
      screen.getByRole("heading", { name: "Implementación" }),
    ).toBeDefined();
    expect(
      screen.getByRole("heading", { name: "Planificación" }),
    ).toBeDefined();
    expect(
      screen.getByText(/no se simula movimiento ni actividad/),
    ).toBeDefined();
  });
});
