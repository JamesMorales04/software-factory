import { describe, expect, it } from "vitest";
import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import {
  buildOfficeSceneLayout,
  cameraStyle,
  clampCamera,
  clampZoom,
  fitCamera,
  OFFICE_MAX_ZOOM,
  OFFICE_MIN_ZOOM,
  panCamera,
  zoomCamera,
  zoomPercent,
} from "./officeSceneLayout";

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
    identity_kind: "PROVIDER_SESSION",
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

function implementers(count: number): ParticipantRecord[] {
  return Array.from({ length: count }, (_, index) =>
    participant(index, "IMPLEMENTER"),
  );
}

function overlaps(
  left: { x: number; y: number; width: number; height: number },
  right: { x: number; y: number; width: number; height: number },
): boolean {
  return (
    left.x < right.x + right.width &&
    right.x < left.x + left.width &&
    left.y < right.y + right.height &&
    right.y < left.y + left.height
  );
}

describe("buildOfficeSceneLayout", () => {
  it.each([0, 1, 5, 12, 30])(
    "lays out %i entities with one slot per distinct identity",
    (count) => {
      const participants = mixedParticipants(count);
      const layout = buildOfficeSceneLayout(participants);
      expect(layout.slots).toHaveLength(count);
      expect(
        new Set(layout.slots.map((slot) => slot.participant.participant_id))
          .size,
      ).toBe(count);
      expect(layout.width).toBeGreaterThan(0);
      expect(layout.height).toBeGreaterThan(0);
      expect(layout.duplicateIds).toEqual([]);
      for (const zone of layout.zones) {
        expect(zone.slots).toHaveLength(
          participants.filter((item) => item.role === zone.role).length,
        );
      }
    },
  );

  it("keeps every station inside the world and non-overlapping", () => {
    for (const count of [0, 1, 5, 12, 30]) {
      const layout = buildOfficeSceneLayout(mixedParticipants(count));
      for (const zone of layout.zones) {
        expect(zone.x).toBeGreaterThanOrEqual(0);
        expect(zone.y).toBeGreaterThanOrEqual(0);
        expect(zone.x + zone.width).toBeLessThanOrEqual(layout.width);
        expect(zone.y + zone.height).toBeLessThanOrEqual(layout.height);
      }
      for (let left = 0; left < layout.zones.length; left += 1) {
        for (let right = left + 1; right < layout.zones.length; right += 1) {
          expect(
            overlaps(layout.zones[left], layout.zones[right]),
            `${layout.zones[left].role} overlaps ${layout.zones[right].role}`,
          ).toBe(false);
        }
      }
    }
  });

  it("keeps stable slots when the same set arrives in another order", () => {
    const participants = mixedParticipants(12);
    const first = buildOfficeSceneLayout(participants);
    const second = buildOfficeSceneLayout([...participants].reverse());
    const seats = new Map(
      first.slots.map((slot) => [
        slot.participant.participant_id,
        `${slot.index}:${slot.column}:${slot.row}:${slot.anchorX}:${slot.anchorY}`,
      ]),
    );
    for (const slot of second.slots) {
      expect(
        `${slot.index}:${slot.column}:${slot.row}:${slot.anchorX}:${slot.anchorY}`,
      ).toBe(seats.get(slot.participant.participant_id));
    }
    expect(second.slots.map((slot) => slot.depth)).toEqual(
      first.slots.map((slot) => slot.depth),
    );
  });

  it("seats 30 implementers without duplicate positions", () => {
    const layout = buildOfficeSceneLayout(implementers(30));
    const implementation = layout.zones.find(
      (zone) => zone.role === "IMPLEMENTER",
    );
    expect(implementation?.slots).toHaveLength(30);
    expect(implementation?.columns).toBe(6);
    expect(implementation?.rows).toBe(5);
    const positions = new Set(
      layout.slots.map(
        (slot) => `${Math.round(slot.anchorX)}:${Math.round(slot.anchorY)}`,
      ),
    );
    expect(positions.size).toBe(30);
  });

  it("renders one avatar per participant identity and reports duplicates", () => {
    const first = participant(0, "IMPLEMENTER");
    const duplicate = {
      ...participant(1, "IMPLEMENTER"),
      participant_id: first.participant_id,
    };
    const layout = buildOfficeSceneLayout([first, duplicate]);
    expect(layout.slots).toHaveLength(1);
    expect(layout.duplicateIds).toEqual([first.participant_id]);
  });

  it("assigns deterministic painter-order depth", () => {
    const layout = buildOfficeSceneLayout(mixedParticipants(12));
    const ordered = [...layout.slots].sort(
      (left, right) =>
        left.anchorY - right.anchorY ||
        left.anchorX - right.anchorX ||
        left.participant.participant_id.localeCompare(
          right.participant.participant_id,
        ),
    );
    expect(ordered.map((slot) => slot.depth)).toEqual(
      layout.slots
        .map((slot) => slot.depth)
        .sort((left, right) => left - right),
    );
  });
});

describe("office camera", () => {
  it("fits a scene inside the viewport and clamps the zoom range", () => {
    const camera = fitCamera(1000, 500, { width: 500, height: 400 });
    expect(camera.zoom).toBe(OFFICE_MIN_ZOOM);
    expect(camera.x).toBe(25);
    expect(camera.y).toBe(88);
    expect(zoomPercent(camera)).toBe(45);
    expect(cameraStyle(camera).transform).toBe(
      "translate(25px, 88px) scale(0.45)",
    );
  });

  it("clamps panning so the scene cannot be lost", () => {
    const camera = clampCamera({ x: 9999, y: -9999, zoom: 1 }, 2000, 1000, {
      width: 500,
      height: 400,
    });
    expect(camera.x).toBe(24);
    expect(camera.y).toBe(400 - 1000 - 24);
  });

  it("limits panning of a scene that already fits", () => {
    const camera = fitCamera(100, 100, { width: 500, height: 400 });
    const panned = panCamera(camera, 10000, 10000, 100, 100, {
      width: 500,
      height: 400,
    });
    expect(panned.x).toBe((500 - 100) / 2 + 48);
    expect(panned.y).toBe((400 - 100) / 2 + 48);
  });

  it("clamps zoom-in and zoom-out around a focus point", () => {
    const camera = { x: 0, y: 0, zoom: 1 };
    const zoomedIn = zoomCamera(camera, 10, 250, 200, 1000, 500, {
      width: 500,
      height: 400,
    });
    expect(zoomedIn.zoom).toBe(OFFICE_MAX_ZOOM);
    const zoomedOut = zoomCamera(camera, 0.001, 250, 200, 1000, 500, {
      width: 500,
      height: 400,
    });
    expect(zoomedOut.zoom).toBe(OFFICE_MIN_ZOOM);
    expect(clampZoom(Number.NaN)).toBe(1);
  });
});
