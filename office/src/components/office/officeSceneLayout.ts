import type { ParticipantRecord, ParticipantRole } from "../../coreApi";

/**
 * Deterministic V0 layout for the isometric Agent Office floor.
 *
 * The scene has no per-frame loop and no physics. Every position is a pure
 * function of the observed participant set, the participant identity and the
 * role zone. A participant's slot is the rank of its stable `participant_id`
 * inside its role group, so repeated renders and reordered Core pages produce
 * identical stations. Adding or removing a participant can renumber later
 * slots in the same role group; the next render is still deterministic.
 */

export const OFFICE_AVATAR_WIDTH = 84;
export const OFFICE_AVATAR_HEIGHT = 100;
export const OFFICE_SLOT_COLUMN = 76;
export const OFFICE_SLOT_ROW = 78;
export const OFFICE_SLOT_STAGGER = 28;
export const OFFICE_ZONE_HEADER = 50;
export const OFFICE_ZONE_PADDING = 14;
export const OFFICE_ZONE_GAP = 34;
export const OFFICE_ZONE_MIN_WIDTH = 190;
export const OFFICE_ZONE_MIN_HEIGHT = 174;
export const OFFICE_WORLD_PADDING = 36;
export const OFFICE_LOOK_COUNT = 48;

export const OFFICE_MIN_ZOOM = 0.45;
export const OFFICE_MAX_ZOOM = 2;
export const OFFICE_ZOOM_STEP = 1.25;
export const OFFICE_PAN_STEP = 120;

export const OFFICE_ROLE_ORDER: readonly ParticipantRole[] = [
  "LEAD",
  "IMPLEMENTER",
  "VERIFIER",
  "REVIEWER",
  "WORKER",
];

export const OFFICE_FALLBACK_VIEWPORT = { width: 960, height: 560 } as const;

const VIEWPORT_PAN_SLACK = 48;
const VIEWPORT_OVERSCROLL = 24;
const FIT_PADDING = 28;
const FIT_MIN_ZOOM = 0.55;
const FIT_MAX_ZOOM = 1.25;

export interface OfficeSlot {
  participant: ParticipantRecord;
  /** Stable rank inside the role group. */
  index: number;
  /** Unique visual appearance index across the scene (0-based). */
  lookIndex: number;
  column: number;
  row: number;
  /** Top-left of the avatar bounding box in world coordinates. */
  x: number;
  y: number;
  /** Floor contact point used for depth ordering. */
  anchorX: number;
  anchorY: number;
  /** 1-based depth order; higher values render in front. */
  depth: number;
}

export interface OfficeZone {
  role: ParticipantRole;
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  columns: number;
  rows: number;
  slots: OfficeSlot[];
}

export interface OfficeSceneLayout {
  width: number;
  height: number;
  zones: OfficeZone[];
  slots: OfficeSlot[];
  /** Participant identities received more than once and rendered only once. */
  duplicateIds: string[];
}

export interface OfficeCamera {
  x: number;
  y: number;
  zoom: number;
}

export interface OfficeViewport {
  width: number;
  height: number;
}

interface ZoneMetrics {
  columns: number;
  rows: number;
  width: number;
  height: number;
}

const ZONE_IDS: Record<ParticipantRole, string> = {
  LEAD: "planning",
  IMPLEMENTER: "implementation",
  VERIFIER: "verification",
  REVIEWER: "review",
  WORKER: "operations",
};

function hashIdentity(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function finite(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

function viewportFor(viewport: OfficeViewport): OfficeViewport {
  return {
    width: Math.max(1, finite(viewport.width, OFFICE_FALLBACK_VIEWPORT.width)),
    height: Math.max(
      1,
      finite(viewport.height, OFFICE_FALLBACK_VIEWPORT.height),
    ),
  };
}

function groupMetrics(memberCount: number): ZoneMetrics {
  const count = Math.max(1, memberCount);
  const columns = Math.min(6, Math.max(1, Math.ceil(Math.sqrt(count))));
  const rows = Math.max(1, Math.ceil(count / columns));
  const contentWidth =
    (columns - 1) * OFFICE_SLOT_COLUMN +
    (rows > 1 ? OFFICE_SLOT_STAGGER : 0) +
    OFFICE_AVATAR_WIDTH;
  const contentHeight = (rows - 1) * OFFICE_SLOT_ROW + OFFICE_AVATAR_HEIGHT;
  return {
    columns,
    rows,
    width: Math.max(
      OFFICE_ZONE_MIN_WIDTH,
      contentWidth + OFFICE_ZONE_PADDING * 2,
    ),
    height: Math.max(
      OFFICE_ZONE_MIN_HEIGHT,
      contentHeight + OFFICE_ZONE_PADDING * 2 + OFFICE_ZONE_HEADER,
    ),
  };
}

function contentOffset(
  zone: { x: number; y: number; width: number; height: number },
  metrics: ZoneMetrics,
): { x: number; y: number } {
  const contentWidth =
    (metrics.columns - 1) * OFFICE_SLOT_COLUMN +
    (metrics.rows > 1 ? OFFICE_SLOT_STAGGER : 0) +
    OFFICE_AVATAR_WIDTH;
  const contentHeight =
    (metrics.rows - 1) * OFFICE_SLOT_ROW + OFFICE_AVATAR_HEIGHT;
  return {
    x: zone.x + (zone.width - contentWidth) / 2,
    y:
      zone.y +
      OFFICE_ZONE_HEADER +
      (zone.height - OFFICE_ZONE_HEADER - contentHeight) / 2,
  };
}

function buildZone(
  role: ParticipantRole,
  members: ParticipantRecord[],
  x: number,
  y: number,
  width: number,
  height: number,
): OfficeZone {
  const sorted = [...members].sort((left, right) =>
    left.participant_id.localeCompare(right.participant_id),
  );
  const metrics = groupMetrics(sorted.length);
  const offset = contentOffset({ x, y, width, height }, metrics);
  const slots: OfficeSlot[] = sorted.map((participant, index) => {
    const column = index % metrics.columns;
    const row = Math.floor(index / metrics.columns);
    const anchorX =
      offset.x +
      column * OFFICE_SLOT_COLUMN +
      (row % 2 === 1 ? OFFICE_SLOT_STAGGER : 0) +
      OFFICE_AVATAR_WIDTH / 2;
    const anchorY = offset.y + row * OFFICE_SLOT_ROW + OFFICE_AVATAR_HEIGHT;
    return {
      participant,
      index,
      lookIndex: 0,
      column,
      row,
      x: anchorX - OFFICE_AVATAR_WIDTH / 2,
      y: anchorY - OFFICE_AVATAR_HEIGHT,
      anchorX,
      anchorY,
      depth: 0,
    };
  });
  return {
    role,
    id: ZONE_IDS[role],
    x,
    y,
    width,
    height,
    columns: metrics.columns,
    rows: metrics.rows,
    slots,
  };
}

/**
 * Lay out five role stations around a central implementation floor.
 *
 * All coordinates are in unscaled world pixels; the camera applies pan/zoom.
 * The layout is intentionally independent of viewport width so that slot
 * identity and camera transforms can be tested separately.
 */
export function buildOfficeSceneLayout(
  participants: ParticipantRecord[],
): OfficeSceneLayout {
  const seen = new Set<string>();
  const duplicateIds: string[] = [];
  const unique = participants.filter((participant) => {
    if (seen.has(participant.participant_id)) {
      if (!duplicateIds.includes(participant.participant_id)) {
        duplicateIds.push(participant.participant_id);
      }
      return false;
    }
    seen.add(participant.participant_id);
    return true;
  });

  const groups = new Map<ParticipantRole, ParticipantRecord[]>();
  for (const role of OFFICE_ROLE_ORDER) groups.set(role, []);
  for (const participant of unique) {
    groups.get(participant.role)?.push(participant);
  }

  const planning = groupMetrics(groups.get("LEAD")?.length ?? 0);
  const implementation = groupMetrics(groups.get("IMPLEMENTER")?.length ?? 0);
  const verification = groupMetrics(groups.get("VERIFIER")?.length ?? 0);
  const review = groupMetrics(groups.get("REVIEWER")?.length ?? 0);
  const operations = groupMetrics(groups.get("WORKER")?.length ?? 0);

  const leftWidth = Math.max(planning.width, operations.width);
  const rightWidth = Math.max(verification.width, review.width);
  const leftHeight = planning.height + OFFICE_ZONE_GAP + operations.height;
  const rightHeight = verification.height + OFFICE_ZONE_GAP + review.height;
  const centerHeight = Math.max(leftHeight, rightHeight, implementation.height);
  const worldWidth =
    OFFICE_WORLD_PADDING * 2 +
    leftWidth +
    OFFICE_ZONE_GAP +
    implementation.width +
    OFFICE_ZONE_GAP +
    rightWidth;
  const worldHeight = OFFICE_WORLD_PADDING * 2 + centerHeight;

  const leftX = OFFICE_WORLD_PADDING;
  const implementationX = leftX + leftWidth + OFFICE_ZONE_GAP;
  const rightX = implementationX + implementation.width + OFFICE_ZONE_GAP;

  const zones: OfficeZone[] = [
    buildZone(
      "LEAD",
      groups.get("LEAD") ?? [],
      leftX,
      OFFICE_WORLD_PADDING,
      leftWidth,
      planning.height,
    ),
    buildZone(
      "IMPLEMENTER",
      groups.get("IMPLEMENTER") ?? [],
      implementationX,
      OFFICE_WORLD_PADDING +
        Math.round((centerHeight - implementation.height) / 2),
      implementation.width,
      implementation.height,
    ),
    buildZone(
      "VERIFIER",
      groups.get("VERIFIER") ?? [],
      rightX,
      OFFICE_WORLD_PADDING,
      rightWidth,
      verification.height,
    ),
    buildZone(
      "REVIEWER",
      groups.get("REVIEWER") ?? [],
      rightX,
      OFFICE_WORLD_PADDING + verification.height + OFFICE_ZONE_GAP,
      rightWidth,
      review.height,
    ),
    buildZone(
      "WORKER",
      groups.get("WORKER") ?? [],
      leftX,
      OFFICE_WORLD_PADDING + planning.height + OFFICE_ZONE_GAP,
      leftWidth,
      operations.height,
    ),
  ];

  const slots = zones.flatMap((zone) => zone.slots);
  const usedLooks = new Set<number>();
  for (const slot of [...slots].sort((left, right) =>
    left.participant.participant_id.localeCompare(
      right.participant.participant_id,
    ),
  )) {
    let lookIndex =
      hashIdentity(slot.participant.participant_id) % OFFICE_LOOK_COUNT;
    while (usedLooks.has(lookIndex)) {
      lookIndex = (lookIndex + 1) % OFFICE_LOOK_COUNT;
    }
    usedLooks.add(lookIndex);
    slot.lookIndex = lookIndex;
  }
  const ordered = [...slots].sort(
    (left, right) =>
      left.anchorY - right.anchorY ||
      left.anchorX - right.anchorX ||
      left.participant.participant_id.localeCompare(
        right.participant.participant_id,
      ),
  );
  ordered.forEach((slot, index) => {
    slot.depth = index + 1;
  });

  return {
    width: Math.round(worldWidth),
    height: Math.round(worldHeight),
    zones,
    slots,
    duplicateIds,
  };
}

export function clampZoom(zoom: number): number {
  return clamp(finite(zoom, 1), OFFICE_MIN_ZOOM, OFFICE_MAX_ZOOM);
}

export function zoomPercent(camera: OfficeCamera): number {
  return Math.round(camera.zoom * 100);
}

export function fitCamera(
  worldWidth: number,
  worldHeight: number,
  viewport: OfficeViewport,
): OfficeCamera {
  const view = viewportFor(viewport);
  const width = Math.max(1, finite(worldWidth, 1));
  const height = Math.max(1, finite(worldHeight, 1));
  const zoom = clamp(
    Math.min(
      (view.width - FIT_PADDING * 2) / width,
      (view.height - FIT_PADDING * 2) / height,
    ),
    FIT_MIN_ZOOM,
    FIT_MAX_ZOOM,
  );
  return {
    x: Math.round((view.width - width * zoom) / 2),
    y: Math.round((view.height - height * zoom) / 2),
    zoom,
  };
}

function clampAxis(value: number, viewport: number, scaled: number): number {
  if (scaled <= viewport) {
    const centered = (viewport - scaled) / 2;
    return Math.round(
      clamp(
        value,
        centered - VIEWPORT_PAN_SLACK,
        centered + VIEWPORT_PAN_SLACK,
      ),
    );
  }
  return Math.round(
    clamp(value, viewport - scaled - VIEWPORT_OVERSCROLL, VIEWPORT_OVERSCROLL),
  );
}

export function clampCamera(
  camera: OfficeCamera,
  worldWidth: number,
  worldHeight: number,
  viewport: OfficeViewport,
): OfficeCamera {
  const view = viewportFor(viewport);
  const zoom = clampZoom(camera.zoom);
  const width = Math.max(1, finite(worldWidth, 1));
  const height = Math.max(1, finite(worldHeight, 1));
  return {
    x: clampAxis(camera.x, view.width, width * zoom),
    y: clampAxis(camera.y, view.height, height * zoom),
    zoom,
  };
}

export function panCamera(
  camera: OfficeCamera,
  deltaX: number,
  deltaY: number,
  worldWidth: number,
  worldHeight: number,
  viewport: OfficeViewport,
): OfficeCamera {
  return clampCamera(
    {
      x: camera.x + finite(deltaX, 0),
      y: camera.y + finite(deltaY, 0),
      zoom: camera.zoom,
    },
    worldWidth,
    worldHeight,
    viewport,
  );
}

export function zoomCamera(
  camera: OfficeCamera,
  factor: number,
  focusX: number,
  focusY: number,
  worldWidth: number,
  worldHeight: number,
  viewport: OfficeViewport,
): OfficeCamera {
  const nextZoom = clampZoom(camera.zoom * finite(factor, 1));
  const ratio = nextZoom / camera.zoom;
  return clampCamera(
    {
      x: focusX - (focusX - camera.x) * ratio,
      y: focusY - (focusY - camera.y) * ratio,
      zoom: nextZoom,
    },
    worldWidth,
    worldHeight,
    viewport,
  );
}

export function cameraStyle(camera: OfficeCamera): {
  transform: string;
  transformOrigin: string;
} {
  return {
    transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})`,
    transformOrigin: "0 0",
  };
}
