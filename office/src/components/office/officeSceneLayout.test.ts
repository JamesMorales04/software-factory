import { describe, expect, it } from "vitest";
import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { officeAvatarLook } from "./officeAvatarAppearance";
import {
  buildOfficeSceneLayout,
  cameraStyle,
  clampCamera,
  clampZoom,
  fitCamera,
  OFFICE_AVATAR_HEIGHT,
  OFFICE_AVATAR_WIDTH,
  OFFICE_CLUSTER_GAP,
  OFFICE_CLUSTER_SIZE,
  OFFICE_MAX_ZOOM,
  OFFICE_MIN_ZOOM,
  OFFICE_SLOT_COLUMN,
  OFFICE_WORLD_PADDING,
  OFFICE_ZONE_SIDE_LANE,
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

  it("keeps every avatar bounding box inside its station and non-overlapping", () => {
    for (const count of [0, 1, 5, 12, 30]) {
      const layout = buildOfficeSceneLayout(mixedParticipants(count));
      for (const zone of layout.zones) {
        for (const slot of zone.slots) {
          expect(slot.x).toBeGreaterThanOrEqual(zone.x);
          expect(slot.y).toBeGreaterThanOrEqual(zone.y);
          expect(slot.x + OFFICE_AVATAR_WIDTH).toBeLessThanOrEqual(
            zone.x + zone.width,
          );
          expect(slot.y + OFFICE_AVATAR_HEIGHT).toBeLessThanOrEqual(
            zone.y + zone.height,
          );
        }
      }
      const bounds = layout.slots.map((slot) => ({
        x: slot.x,
        y: slot.y,
        width: OFFICE_AVATAR_WIDTH,
        height: OFFICE_AVATAR_HEIGHT,
      }));
      for (let left = 0; left < bounds.length; left += 1) {
        for (let right = left + 1; right < bounds.length; right += 1) {
          expect(
            overlaps(bounds[left], bounds[right]),
            `avatars ${left} and ${right} overlap at ${count} entities`,
          ).toBe(false);
        }
      }
    }

    const dense = buildOfficeSceneLayout(implementers(30));
    const denseBounds = dense.slots.map((slot) => ({
      x: slot.x,
      y: slot.y,
      width: OFFICE_AVATAR_WIDTH,
      height: OFFICE_AVATAR_HEIGHT,
    }));
    for (let left = 0; left < denseBounds.length; left += 1) {
      for (let right = left + 1; right < denseBounds.length; right += 1) {
        expect(
          overlaps(denseBounds[left], denseBounds[right]),
          `avatars ${left} and ${right} overlap at 30 implementers`,
        ).toBe(false);
      }
    }
  });

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

  it("gives five implementers five distinct visual identities", () => {
    const layout = buildOfficeSceneLayout(implementers(5));
    const zone = layout.zones.find((item) => item.role === "IMPLEMENTER");
    expect(zone?.slots).toHaveLength(5);
    expect(zone?.rows).toBe(2);
    expect(zone?.columns).toBe(3);
    expect(zone?.slots.map((slot) => slot.row)).toEqual([0, 0, 0, 1, 1]);
    expect(
      zone?.slots.slice(0, 3).every((slot) => slot.perspectiveScale === 0.9),
    ).toBe(true);
    expect(
      zone?.slots.slice(3).every((slot) => slot.perspectiveScale === 1),
    ).toBe(true);
    for (let left = 0; left < (zone?.slots.length ?? 0); left += 1) {
      for (
        let right = left + 1;
        right < (zone?.slots.length ?? 0);
        right += 1
      ) {
        const leftSlot = zone?.slots[left];
        const rightSlot = zone?.slots[right];
        if (!leftSlot || !rightSlot) continue;
        expect(
          overlaps(
            {
              x: leftSlot.x,
              y: leftSlot.y,
              width: OFFICE_AVATAR_WIDTH,
              height: OFFICE_AVATAR_HEIGHT,
            },
            {
              x: rightSlot.x,
              y: rightSlot.y,
              width: OFFICE_AVATAR_WIDTH,
              height: OFFICE_AVATAR_HEIGHT,
            },
          ),
        ).toBe(false);
      }
    }
    const looks = new Set(
      (zone?.slots ?? []).map(
        (slot) => officeAvatarLook(slot.lookIndex).signature,
      ),
    );
    expect(looks.size).toBe(5);
  });

  it("terminates with distinct identities after the former 48-look boundary", () => {
    const layout = buildOfficeSceneLayout(mixedParticipants(200));
    expect(layout.slots).toHaveLength(200);
    expect(new Set(layout.slots.map((slot) => slot.lookIndex)).size).toBe(200);
    expect(
      new Set(
        layout.slots.map((slot) => officeAvatarLook(slot.lookIndex).signature),
      ).size,
    ).toBe(200);
  });

  it("seats 30 implementers without duplicate positions", () => {
    const layout = buildOfficeSceneLayout(implementers(30));
    const implementation = layout.zones.find(
      (zone) => zone.role === "IMPLEMENTER",
    );
    expect(implementation?.slots).toHaveLength(30);
    expect(implementation?.columns).toBe(8);
    expect(implementation?.rows).toBe(4);
    const positions = new Set(
      layout.slots.map(
        (slot) => `${Math.round(slot.anchorX)}:${Math.round(slot.anchorY)}`,
      ),
    );
    expect(positions.size).toBe(30);
    const looks = new Set(
      layout.slots.map((slot) => officeAvatarLook(slot.lookIndex).signature),
    );
    expect(looks.size).toBe(30);
  });

  it("groups workstation clusters around real aisles", () => {
    const layout = buildOfficeSceneLayout(implementers(30));
    const implementation = layout.zones.find(
      (zone) => zone.role === "IMPLEMENTER",
    );
    const anchors = implementation?.slots ?? [];
    const columns = [...new Set(anchors.map((slot) => slot.column))].sort(
      (left, right) => left - right,
    );
    const rows = [...new Set(anchors.map((slot) => slot.row))].sort(
      (left, right) => left - right,
    );
    expect(columns).toHaveLength(8);
    expect(rows).toHaveLength(4);
    const columnAnchors = columns.map(
      (column) => anchors.find((slot) => slot.column === column)?.anchorX ?? 0,
    );
    const rowAnchors = rows.map(
      (row) => anchors.find((slot) => slot.row === row)?.anchorY ?? 0,
    );
    const clusteredColumn =
      columnAnchors[OFFICE_CLUSTER_SIZE] -
      columnAnchors[OFFICE_CLUSTER_SIZE - 1];
    const plainColumn = columnAnchors[1] - columnAnchors[0];
    expect(clusteredColumn).toBeGreaterThan(
      plainColumn + OFFICE_CLUSTER_GAP - 1,
    );
    expect(rowAnchors.at(-1)).toBeGreaterThan(rowAnchors[0] ?? 0);
    expect(new Set(columnAnchors).size).toBe(8);
    expect(new Set(rowAnchors).size).toBe(4);
    expect(OFFICE_SLOT_COLUMN).toBeGreaterThan(OFFICE_AVATAR_WIDTH);
  });

  it("reserves a furniture lane that avatars never occupy", () => {
    const layout = buildOfficeSceneLayout(implementers(30));
    const implementation = layout.zones.find(
      (zone) => zone.role === "IMPLEMENTER",
    );
    expect(implementation).toBeDefined();
    for (const slot of implementation?.slots ?? []) {
      expect(slot.x + OFFICE_AVATAR_WIDTH).toBeLessThanOrEqual(
        (implementation?.x ?? 0) +
          (implementation?.width ?? 0) -
          OFFICE_ZONE_SIDE_LANE +
          1,
      );
    }
  });

  it("collapses stations without a recorded participant into one studio", () => {
    const layout = buildOfficeSceneLayout(implementers(30));
    expect(layout.zones).toHaveLength(1);
    const [implementation] = layout.zones;
    expect(implementation?.role).toBe("IMPLEMENTER");
    expect(implementation?.wing).toBe("center");
    expect(layout.emptyRoles).toEqual([
      "LEAD",
      "VERIFIER",
      "REVIEWER",
      "WORKER",
    ]);
    if (!implementation) throw new Error("implementation station is missing");
    expect(layout.width).toBeLessThan(
      implementation.width + OFFICE_WORLD_PADDING * 3,
    );
  });

  it("keeps recorded wings and omits an empty center column", () => {
    const layout = buildOfficeSceneLayout([
      participant(0, "LEAD"),
      participant(1, "VERIFIER"),
    ]);
    expect(layout.zones.map((zone) => zone.role)).toEqual(["LEAD", "VERIFIER"]);
    expect(layout.emptyRoles).toContain("IMPLEMENTER");
    const [left, right] = layout.zones;
    expect(left?.wing).toBe("left");
    expect(right?.wing).toBe("right");
    expect(left?.x).toBeLessThan(right?.x ?? 0);
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

  it("resolves conflicting duplicate identities deterministically", () => {
    const id = "implementer:participant-00";
    const current = participant(0, "IMPLEMENTER", {
      participant_id: id,
      observation: "CURRENT",
      status: "ACTIVE",
      last_observed_at: "2026-10-28T10:01:00Z",
    });
    const final = participant(0, "LEAD", {
      participant_id: id,
      observation: "FINAL",
      status: "COMPLETED",
      last_observed_at: "2026-10-28T10:09:00Z",
    });
    const stale = participant(0, "VERIFIER", {
      participant_id: id,
      observation: "STALE",
      status: "ACTIVE",
      last_observed_at: "2026-10-28T10:08:00Z",
    });
    const first = buildOfficeSceneLayout([final, current, stale]);
    const second = buildOfficeSceneLayout([stale, current, final]);
    expect(first.slots).toHaveLength(1);
    expect(second.slots).toHaveLength(1);
    const winner = first.slots[0];
    expect(winner.participant.observation).toBe("CURRENT");
    expect(winner.participant.role).toBe("IMPLEMENTER");
    expect(winner.participant.status).toBe("ACTIVE");
    expect(first.duplicateIds).toEqual([id]);
    expect(second.duplicateIds).toEqual([id]);
    expect({
      role: second.slots[0].participant.role,
      observation: second.slots[0].participant.observation,
      status: second.slots[0].participant.status,
      lookIndex: second.slots[0].lookIndex,
      x: second.slots[0].x,
      y: second.slots[0].y,
    }).toEqual({
      role: winner.participant.role,
      observation: winner.participant.observation,
      status: winner.participant.status,
      lookIndex: winner.lookIndex,
      x: winner.x,
      y: winner.y,
    });
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
    expect(camera.zoom).toBeCloseTo(0.472);
    expect(camera.x).toBe(14);
    expect(camera.y).toBe(82);
    expect(zoomPercent(camera)).toBe(47);
    expect(cameraStyle(camera).transform).toBe(
      "translate(14px, 82px) scale(0.472)",
    );
  });

  it("fits the studio width on a narrow mobile viewport", () => {
    const camera = fitCamera(928, 574, { width: 356, height: 340 });
    expect(camera.zoom).toBeCloseTo(328 / 928);
    expect(camera.x).toBe(14);
    expect(928 * camera.zoom).toBeLessThanOrEqual(356 - 28);
  });

  it("clamps panning so the scene cannot be lost", () => {
    const camera = clampCamera({ x: 9999, y: -9999, zoom: 1 }, 2000, 1000, {
      width: 500,
      height: 400,
    });
    expect(camera.x).toBe(24);
    expect(camera.y).toBe(400 - 1000 - 24);
  });

  it("anchors a scene taller than the viewport below the studio wall", () => {
    const camera = fitCamera(220, 1800, { width: 420, height: 320 });
    expect(camera.zoom).toBe(0.2);
    expect(camera.x).toBe(Math.round((420 - 220 * 0.2) / 2));
    expect(camera.y).toBe(8);
  });

  it("limits panning of a scene that already fits", () => {
    const camera = fitCamera(100, 100, { width: 500, height: 400 });
    const panned = panCamera(camera, 10000, 10000, 100, 100, {
      width: 500,
      height: 400,
    });
    expect(panned.x).toBe(198);
    expect(panned.y).toBe(148);
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
