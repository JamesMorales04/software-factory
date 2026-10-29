import type { ParticipantRecord, ParticipantRole } from "../../coreApi";

/**
 * Deterministic planar (2.5D) layout for the isometric Agent Office studio.
 *
 * The scene has no per-frame loop and no physics. Every position is a pure
 * function of the observed participant set, the participant identity and the
 * role station. A participant's slot is the rank of its stable `participant_id`
 * inside its role group, so repeated renders and reordered Core pages produce
 * identical stations. Adding or removing a participant can renumber later
 * slots in the same role group; the next render is still deterministic.
 *
 * V3 composes a single continuous studio deck: only stations with recorded
 * participants become floor areas, so a 30-Implementer projection is one wide
 * bullpen instead of five empty boxes. Empty role stations are reported
 * separately and never rendered as furniture or fabricated agents.
 */

export const OFFICE_AVATAR_WIDTH = 92;
export const OFFICE_AVATAR_HEIGHT = 112;
/**
 * Pitch is deliberately larger than the avatar footprint. A slot owns its
 * complete button bounds, so two slots must never overlap even when the
 * depth painter later draws one closer to the viewer.
 *
 * Seats are grouped in workstation clusters of `OFFICE_CLUSTER_SIZE` columns
 * and rows. The cluster gap is a real aisle in the floor plan, not a visual
 * offset: the layout math below includes it in both the content size and the
 * slot coordinates so the aisle remains empty at every density.
 */
export const OFFICE_SLOT_COLUMN = 110;
export const OFFICE_SLOT_ROW = 118;
export const OFFICE_SLOT_STAGGER = 24;
export const OFFICE_CLUSTER_SIZE = 3;
export const OFFICE_CLUSTER_GAP = 26;
export const OFFICE_ZONE_HEADER = 72;
export const OFFICE_ZONE_PADDING = 14;
/** Reserved floor lane for role furniture; avatars never enter it. */
export const OFFICE_ZONE_SIDE_LANE = 72;
export const OFFICE_ZONE_GAP = 34;
export const OFFICE_ZONE_MIN_WIDTH = 260;
export const OFFICE_ZONE_MIN_HEIGHT = 204;
export const OFFICE_WORLD_PADDING = 40;
/** Vertical space reserved above the floor for the continuous studio wall. */
export const OFFICE_TOP_PADDING = 76;
export const OFFICE_WALL_HEIGHT = 66;
export const OFFICE_MIN_ZOOM = 0.2;
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

/** Left and right wings of the studio plan, top-to-bottom. */
export const OFFICE_LEFT_ROLES: readonly ParticipantRole[] = ["LEAD", "WORKER"];
export const OFFICE_RIGHT_ROLES: readonly ParticipantRole[] = [
  "VERIFIER",
  "REVIEWER",
];

export const OFFICE_FALLBACK_VIEWPORT = { width: 960, height: 560 } as const;

const VIEWPORT_PAN_SLACK = 48;
const VIEWPORT_OVERSCROLL = 24;
const FIT_PADDING = 14;
const FIT_MIN_ZOOM = OFFICE_MIN_ZOOM;
const FIT_MAX_ZOOM = 2;

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
  /** Figure scale from the back to front row of its workstation cluster. */
  perspectiveScale: number;
}

export interface OfficeZone {
  role: ParticipantRole;
  id: string;
  /** Stable plan side; drives partition and background composition. */
  wing: "left" | "center" | "right";
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
  /** Top edge of the walkable floor, below the continuous studio wall. */
  floorTop: number;
  zones: OfficeZone[];
  slots: OfficeSlot[];
  /** Roles with no recorded participant in this projection. */
  emptyRoles: ParticipantRole[];
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

function clusterOffset(index: number, pitch: number): number {
  return (
    index * pitch +
    Math.floor(Math.max(0, index) / OFFICE_CLUSTER_SIZE) * OFFICE_CLUSTER_GAP
  );
}

function groupMetrics(memberCount: number): ZoneMetrics {
  const count = Math.max(1, memberCount);
  // One or two participants share a compact bench. Larger crews use a
  // staggered two-row pod, then grow as a wide bullpen with at most eight
  // columns. Row depth gives the SVG scene a back/front cue without changing
  // deterministic identity placement.
  const rows =
    count <= 2 ? 1 : count <= 12 ? 2 : Math.max(1, Math.ceil(count / 8));
  const columns = Math.min(8, count, Math.max(1, Math.ceil(count / rows)));
  const contentWidth =
    clusterOffset(columns - 1, OFFICE_SLOT_COLUMN) +
    (rows > 1 ? OFFICE_SLOT_STAGGER : 0) +
    OFFICE_AVATAR_WIDTH;
  const contentHeight =
    clusterOffset(rows - 1, OFFICE_SLOT_ROW) + OFFICE_AVATAR_HEIGHT;
  return {
    columns,
    rows,
    width: Math.max(
      OFFICE_ZONE_MIN_WIDTH,
      contentWidth + OFFICE_ZONE_PADDING * 2 + OFFICE_ZONE_SIDE_LANE,
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
    clusterOffset(metrics.columns - 1, OFFICE_SLOT_COLUMN) +
    (metrics.rows > 1 ? OFFICE_SLOT_STAGGER : 0) +
    OFFICE_AVATAR_WIDTH;
  const contentHeight =
    clusterOffset(metrics.rows - 1, OFFICE_SLOT_ROW) + OFFICE_AVATAR_HEIGHT;
  const seatAreaWidth = Math.max(
    contentWidth,
    zone.width - OFFICE_ZONE_SIDE_LANE,
  );
  return {
    x: zone.x + (seatAreaWidth - contentWidth) / 2,
    y:
      zone.y +
      OFFICE_ZONE_HEADER +
      (zone.height - OFFICE_ZONE_HEADER - contentHeight) / 2,
  };
}

function buildZone(
  role: ParticipantRole,
  wing: OfficeZone["wing"],
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
      clusterOffset(column, OFFICE_SLOT_COLUMN) +
      (row % 2 === 1 ? OFFICE_SLOT_STAGGER : 0) +
      OFFICE_AVATAR_WIDTH / 2;
    const anchorY =
      offset.y + clusterOffset(row, OFFICE_SLOT_ROW) + OFFICE_AVATAR_HEIGHT;
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
      perspectiveScale:
        metrics.rows > 1 ? 0.9 + (0.1 * row) / (metrics.rows - 1) : 1,
    };
  });
  return {
    role,
    id: ZONE_IDS[role],
    wing,
    x,
    y,
    width,
    height,
    columns: metrics.columns,
    rows: metrics.rows,
    slots,
  };
}

const DUPLICATE_OBSERVATION_PRIORITY: Record<string, number> = {
  CURRENT: 0,
  STALE: 1,
  FINAL: 2,
  NOT_EXPOSED: 3,
  UNKNOWN: 4,
};

function duplicateRecordKey(participant: ParticipantRecord): string {
  return [
    participant.role,
    participant.identity_kind,
    participant.identity_id,
    participant.principal_id ?? "",
    participant.project_id ?? "",
    participant.action_id ?? "",
    participant.campaign_id ?? "",
    participant.request_id ?? "",
    participant.work_unit_id ?? "",
    participant.run_id ?? "",
    participant.attempt_id ?? "",
    participant.worker_id ?? "",
    participant.status,
    participant.status_source,
    participant.lease_state,
    participant.observation,
    participant.session?.session_record_id ?? "",
    participant.failure_reason ?? "",
    participant.observation_note,
    participant.recorded_at,
    participant.last_observed_at,
  ].join("\u0000");
}

/**
 * Deterministic winner for duplicate projections of the same participant_id.
 *
 * A live observation beats an unresolved or terminal one; among equal
 * observations the newest recorded observation wins. The remaining fields are
 * a total lexical tiebreak so reversing the API page cannot change the station,
 * state or appearance rendered for an identity.
 */
export function compareDuplicateRecords(
  left: ParticipantRecord,
  right: ParticipantRecord,
): number {
  const leftPriority =
    DUPLICATE_OBSERVATION_PRIORITY[left.observation] ?? Number.MAX_SAFE_INTEGER;
  const rightPriority =
    DUPLICATE_OBSERVATION_PRIORITY[right.observation] ??
    Number.MAX_SAFE_INTEGER;
  if (leftPriority !== rightPriority) return leftPriority - rightPriority;
  const recency = right.last_observed_at.localeCompare(left.last_observed_at);
  if (recency !== 0) return recency;
  return duplicateRecordKey(left).localeCompare(duplicateRecordKey(right));
}

interface StudioColumn {
  wing: OfficeZone["wing"];
  roles: ParticipantRole[];
  width: number;
  height: number;
  x: number;
}

/**
 * Lay out the recorded role stations as one continuous studio.
 *
 * Columns only exist when they contain a recorded station. The center column
 * holds Implementation; the wings hold Planning/Operations and
 * Verification/Review. The world grows with the widest and tallest present
 * column and is centered by a fitted camera. All coordinates are unscaled
 * world pixels.
 */
export function buildOfficeSceneLayout(
  participants: ParticipantRecord[],
): OfficeSceneLayout {
  const byId = new Map<string, ParticipantRecord>();
  const duplicateSet = new Set<string>();
  for (const participant of participants) {
    const current = byId.get(participant.participant_id);
    if (!current) {
      byId.set(participant.participant_id, participant);
      continue;
    }
    duplicateSet.add(participant.participant_id);
    if (compareDuplicateRecords(participant, current) < 0) {
      byId.set(participant.participant_id, participant);
    }
  }
  const duplicateIds = [...duplicateSet].sort((left, right) =>
    left.localeCompare(right),
  );
  const unique = [...byId.values()].sort((left, right) =>
    left.participant_id.localeCompare(right.participant_id),
  );

  const groups = new Map<ParticipantRole, ParticipantRecord[]>();
  for (const role of OFFICE_ROLE_ORDER) groups.set(role, []);
  for (const participant of unique) {
    groups.get(participant.role)?.push(participant);
  }
  const members = (role: ParticipantRole) => groups.get(role) ?? [];
  const metrics = (role: ParticipantRole) => groupMetrics(members(role).length);
  const emptyRoles = OFFICE_ROLE_ORDER.filter(
    (role) => members(role).length === 0,
  );

  const planning = metrics("LEAD");
  const implementation = metrics("IMPLEMENTER");
  const verification = metrics("VERIFIER");
  const review = metrics("REVIEWER");
  const operations = metrics("WORKER");

  const wings: {
    wing: OfficeZone["wing"];
    roles: ParticipantRole[];
    metric: ZoneMetrics;
  }[] = [];
  const leftRoles = OFFICE_LEFT_ROLES.filter(
    (role) => members(role).length > 0,
  );
  const rightRoles = OFFICE_RIGHT_ROLES.filter(
    (role) => members(role).length > 0,
  );
  if (leftRoles.length > 0) {
    wings.push({
      wing: "left",
      roles: leftRoles,
      metric: planning.height >= operations.height ? planning : operations,
    });
  }
  if (members("IMPLEMENTER").length > 0) {
    wings.push({
      wing: "center",
      roles: ["IMPLEMENTER"],
      metric: implementation,
    });
  }
  if (rightRoles.length > 0) {
    wings.push({
      wing: "right",
      roles: rightRoles,
      metric: verification.height >= review.height ? verification : review,
    });
  }

  const columns: StudioColumn[] = [];
  let cursorX = OFFICE_WORLD_PADDING;
  for (const wing of wings) {
    const heights = wing.roles.map((role) => metrics(role).height);
    const height =
      heights.reduce((total, value) => total + value, 0) +
      Math.max(0, heights.length - 1) * OFFICE_ZONE_GAP;
    const width = Math.max(
      wing.metric.width,
      ...wing.roles.map((role) => metrics(role).width),
    );
    columns.push({
      wing: wing.wing,
      roles: wing.roles,
      width,
      height,
      x: cursorX,
    });
    cursorX += width + OFFICE_ZONE_GAP;
  }

  const contentHeight = Math.max(1, ...columns.map((column) => column.height));
  const floorTop = OFFICE_TOP_PADDING;
  const worldWidth =
    columns.length === 0
      ? OFFICE_WORLD_PADDING * 2 + OFFICE_ZONE_MIN_WIDTH
      : cursorX - OFFICE_ZONE_GAP + OFFICE_WORLD_PADDING;
  const worldHeight = floorTop + contentHeight + OFFICE_WORLD_PADDING;

  const zones: OfficeZone[] = [];
  for (const column of columns) {
    let cursorY = floorTop;
    if (column.roles.length === 1 && column.wing === "center") {
      const only = metrics(column.roles[0]).height;
      cursorY = floorTop + Math.round((contentHeight - only) / 2);
    }
    for (const role of column.roles) {
      const metric = metrics(role);
      zones.push(
        buildZone(
          role,
          column.wing,
          members(role),
          column.x,
          cursorY,
          column.width,
          metric.height,
        ),
      );
      cursorY += metric.height + OFFICE_ZONE_GAP;
    }
  }

  const slots = zones.flatMap((zone) => zone.slots);
  // Assigning in stable identity order keeps the same set deterministic and
  // gives every participant a unique palette combination without collision
  // probing. The mixed-radix appearance palette supports far more combinations
  // than the current Office projection's 200-participant page size.
  [...slots]
    .sort((left, right) =>
      left.participant.participant_id.localeCompare(
        right.participant.participant_id,
      ),
    )
    .forEach((slot, index) => {
      slot.lookIndex = index;
    });
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
    floorTop,
    zones,
    slots,
    emptyRoles,
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
  const scaledWidth = width * zoom;
  const scaledHeight = height * zoom;
  return {
    x: Math.round((view.width - scaledWidth) / 2),
    // A tall scene that cannot fit vertically is anchored below the studio's
    // top wall instead of being cropped equally at both ends. The operator
    // still pans; this keeps room plaques and the top row visible on narrow
    // viewports instead of opening on a mid-floor crop.
    y:
      scaledHeight <= view.height
        ? Math.round((view.height - scaledHeight) / 2)
        : 8,
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
