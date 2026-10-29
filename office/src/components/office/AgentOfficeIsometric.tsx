import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Maximize2,
  Minus,
  Plus,
} from "lucide-react";
import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { type MessageKey, useI18n } from "../../i18n";
import { Button, IconButton } from "../ui/Button";
import { cn } from "../ui/cn";
import { EmptyState, Panel } from "../ui/Surface";
import {
  buildOfficeSceneLayout,
  cameraStyle,
  clampCamera,
  fitCamera,
  OFFICE_AVATAR_HEIGHT,
  OFFICE_AVATAR_WIDTH,
  OFFICE_FALLBACK_VIEWPORT,
  OFFICE_MAX_ZOOM,
  OFFICE_MIN_ZOOM,
  OFFICE_PAN_STEP,
  OFFICE_ZONE_HEADER,
  OFFICE_ZOOM_STEP,
  type OfficeCamera,
  type OfficeSlot,
  type OfficeViewport,
  type OfficeZone,
  panCamera,
  zoomCamera,
  zoomPercent,
} from "./officeSceneLayout";
import { observationLabel, roleLabel } from "./participantLabels";

const ZONE_META: Record<
  ParticipantRole,
  { labelKey: MessageKey; descriptionKey: MessageKey }
> = {
  LEAD: {
    labelKey: "agentOffice.zone.planning",
    descriptionKey: "agentOffice.zone.planningDescription",
  },
  IMPLEMENTER: {
    labelKey: "agentOffice.zone.implementation",
    descriptionKey: "agentOffice.zone.implementationDescription",
  },
  VERIFIER: {
    labelKey: "agentOffice.zone.verification",
    descriptionKey: "agentOffice.zone.verificationDescription",
  },
  REVIEWER: {
    labelKey: "agentOffice.zone.review",
    descriptionKey: "agentOffice.zone.reviewDescription",
  },
  WORKER: {
    labelKey: "agentOffice.zone.operations",
    descriptionKey: "agentOffice.zone.operationsDescription",
  },
};

const OBSERVATION_GLYPH: Record<string, string> = {
  CURRENT: "●",
  STALE: "◐",
  FINAL: "■",
  NOT_EXPOSED: "◇",
  UNKNOWN: "?",
};

interface AvatarLook {
  skin: number;
  hair: number;
  shirt: number;
}

function hashIdentity(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

function identityLabel(value: string): string {
  if (value.length <= 9) return value;
  return `…${value.slice(-7)}`;
}

function avatarLook(participantId: string): AvatarLook {
  const hash = hashIdentity(participantId);
  return {
    skin: (hash % 3) + 1,
    hair: (Math.floor(hash / 3) % 3) + 1,
    shirt: (Math.floor(hash / 9) % 3) + 1,
  };
}

function figureVariables(look: AvatarLook): CSSProperties {
  return {
    "--office-skin": `var(--office-skin-${look.skin})`,
    "--office-hair": `var(--office-hair-${look.hair})`,
    "--office-shirt": `var(--office-shirt-${look.shirt})`,
  } as CSSProperties;
}

function focusNeighbor(
  slot: OfficeSlot,
  key: string,
  slots: OfficeSlot[],
  refs: Map<string, HTMLButtonElement>,
) {
  const candidates = slots
    .filter(
      (candidate) =>
        candidate.participant.participant_id !==
        slot.participant.participant_id,
    )
    .map((candidate) => ({
      candidate,
      dx: candidate.anchorX - slot.anchorX,
      dy: candidate.anchorY - slot.anchorY,
    }))
    .filter(({ dx, dy }) => {
      if (key === "ArrowRight") return dx > 0 && Math.abs(dy) <= Math.abs(dx);
      if (key === "ArrowLeft") return dx < 0 && Math.abs(dy) <= Math.abs(dx);
      if (key === "ArrowDown") return dy > 0 && Math.abs(dx) <= Math.abs(dy);
      if (key === "ArrowUp") return dy < 0 && Math.abs(dx) <= Math.abs(dy);
      return false;
    })
    .sort(
      (left, right) =>
        Math.hypot(left.dx, left.dy) - Math.hypot(right.dx, right.dy),
    );
  const next = candidates[0]?.candidate;
  if (next) {
    refs.get(next.participant.participant_id)?.focus();
  }
}

function OfficeAvatar({
  slot,
  offsetX,
  offsetY,
  selected,
  onSelect,
  registerRef,
  slots,
  refs,
}: {
  slot: OfficeSlot;
  offsetX: number;
  offsetY: number;
  selected: boolean;
  onSelect: (participantId: string) => void;
  registerRef: (
    participantId: string,
    element: HTMLButtonElement | null,
  ) => void;
  slots: OfficeSlot[];
  refs: Map<string, HTMLButtonElement>;
}) {
  const { t } = useI18n();
  const { participant } = slot;
  const observation = observationLabel(participant);
  const glyph = OBSERVATION_GLYPH[participant.observation] ?? "?";
  const role = roleLabel(participant.role, t);

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (
      ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"].includes(event.key)
    ) {
      event.preventDefault();
      focusNeighbor(slot, event.key, slots, refs);
    }
  }

  return (
    <button
      ref={(element) => registerRef(participant.participant_id, element)}
      type="button"
      className={cn("office-avatar", selected && "is-selected")}
      style={{
        ...figureVariables(avatarLook(participant.participant_id)),
        left: slot.x - offsetX,
        top: slot.y - offsetY,
        width: OFFICE_AVATAR_WIDTH,
        height: OFFICE_AVATAR_HEIGHT,
        zIndex: selected ? 200 : 10 + slot.depth,
      }}
      aria-pressed={selected}
      aria-label={t("agentOffice.card.inspect", {
        role,
        id: participant.participant_id,
        status: participant.status,
        observation,
      })}
      onClick={() => onSelect(participant.participant_id)}
      onKeyDown={onKeyDown}
      data-office-avatar=""
      data-testid={`office-avatar-${participant.participant_id}`}
      data-participant-id={participant.participant_id}
      data-role={participant.role}
      data-observation={participant.observation}
      data-slot-index={slot.index}
      data-column={slot.column}
      data-row={slot.row}
      data-anchor-x={Math.round(slot.anchorX)}
      data-anchor-y={Math.round(slot.anchorY)}
    >
      <span className="office-avatar-label">
        <code title={participant.identity_id}>
          {identityLabel(participant.identity_id)}
        </code>
      </span>
      <svg
        className="office-avatar-figure"
        viewBox="0 0 72 84"
        aria-hidden="true"
        focusable="false"
      >
        <ellipse
          className="office-avatar-shadow"
          cx="36"
          cy="76.5"
          rx="23"
          ry="4.5"
        />
        <rect
          className="office-avatar-chair"
          x="26"
          y="64"
          width="20"
          height="9"
          rx="4"
        />
        <path
          className="office-avatar-leg office-avatar-leg-left"
          d="M31 54 L30 71"
        />
        <path
          className="office-avatar-leg office-avatar-leg-right"
          d="M42 54 L43 71"
        />
        <path className="office-avatar-shoe" d="M26 73 L34 73" />
        <path className="office-avatar-shoe" d="M39 73 L47 73" />
        <path
          className="office-avatar-torso"
          d="M25 40 q11 -7 23 0 l3 21 q-15 6 -30 0 z"
        />
        <path className="office-avatar-arm" d="M27 42 q-6 9 -3 20" />
        <path className="office-avatar-arm" d="M46 42 q6 9 3 20" />
        <circle className="office-avatar-hand" cx="24" cy="63" r="3" />
        <circle className="office-avatar-hand" cx="49" cy="63" r="3" />
        <rect
          className="office-avatar-neck"
          x="33"
          y="31"
          width="7"
          height="8"
          rx="2.5"
        />
        <ellipse
          className="office-avatar-head"
          cx="36.5"
          cy="24"
          rx="8.2"
          ry="9.4"
        />
        <circle className="office-avatar-eye" cx="33.6" cy="24.5" r="0.9" />
        <circle className="office-avatar-eye" cx="39.4" cy="24.5" r="0.9" />
        <path
          className="office-avatar-hair"
          d="M28.2 23.6 q-0.6 -9.6 8.3 -9.6 q9 0 8.3 9.6 q-3.2 -5.2 -8.3 -5.2 q-5.1 0 -8.3 5.2 z"
        />
        <path
          className="office-avatar-desk-top"
          d="M10 72 L36 64 L62 72 L36 81 Z"
        />
        <path
          className="office-avatar-desk-side"
          d="M10 72 L36 81 L36 85 L10 76 Z"
        />
        <path
          className="office-avatar-desk-side office-avatar-desk-side-right"
          d="M36 81 L62 72 L62 76 L36 85 Z"
        />
        <rect
          className="office-avatar-monitor"
          x="19"
          y="53"
          width="13"
          height="9"
          rx="1.5"
        />
        <rect
          className="office-avatar-screen"
          x="21"
          y="55"
          width="9"
          height="5"
          rx="0.8"
        />
        <path className="office-avatar-monitor-stand" d="M25 62 L25 65" />
        <circle
          className="office-avatar-lamp"
          cx="58"
          cy="69"
          r="2.6"
          data-observation={participant.observation}
        />
      </svg>
      <span
        className="office-avatar-status"
        data-observation={participant.observation}
        aria-hidden="true"
      >
        {glyph}
      </span>
    </button>
  );
}

function OfficeZonePad({ zone }: { zone: OfficeZone }) {
  const left = 8;
  const right = zone.width - 8;
  const top = OFFICE_ZONE_HEADER - 2;
  const bottom = zone.height - 6;
  const cut = 18;
  const topPoints = [
    `${left + cut},${top}`,
    `${right - cut},${top}`,
    `${right},${top + cut}`,
    `${right},${bottom - cut}`,
    `${right - cut},${bottom}`,
    `${left + cut},${bottom}`,
    `${left},${bottom - cut}`,
    `${left},${top + cut}`,
  ].join(" ");
  const sidePoints = topPoints
    .split(" ")
    .map((point) => {
      const [x, y] = point.split(",");
      return `${x},${Number(y) + 9}`;
    })
    .join(" ");
  return (
    <svg
      className="office-zone-pad"
      viewBox={`0 0 ${zone.width} ${zone.height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <polygon className="office-zone-pad-side" points={sidePoints} />
      <polygon className="office-zone-pad-top" points={topPoints} />
    </svg>
  );
}

function OfficeZoneStation({
  zone,
  selectedId,
  onSelect,
  slots,
  refs,
  registerRef,
}: {
  zone: OfficeZone;
  selectedId: string | null;
  onSelect: (participantId: string) => void;
  slots: OfficeSlot[];
  refs: Map<string, HTMLButtonElement>;
  registerRef: (
    participantId: string,
    element: HTMLButtonElement | null,
  ) => void;
}) {
  const { t } = useI18n();
  const meta = ZONE_META[zone.role];
  const headingId = `office-zone-${zone.id}`;
  return (
    <section
      className={cn("office-zone", `office-zone-${zone.id}`)}
      aria-labelledby={headingId}
      style={{
        left: zone.x,
        top: zone.y,
        width: zone.width,
        height: zone.height,
        zIndex: 2,
      }}
    >
      <OfficeZonePad zone={zone} />
      <header className="office-zone-header">
        <h3 id={headingId} title={t(meta.descriptionKey)}>
          {t(meta.labelKey)}
        </h3>
        <span className="office-zone-count">
          {t("agentOffice.zone.recorded", { count: zone.slots.length })}
        </span>
      </header>
      {zone.slots.map((slot) => (
        <OfficeAvatar
          key={slot.participant.participant_id}
          slot={slot}
          offsetX={zone.x}
          offsetY={zone.y}
          selected={slot.participant.participant_id === selectedId}
          onSelect={onSelect}
          registerRef={registerRef}
          slots={slots}
          refs={refs}
        />
      ))}
    </section>
  );
}

export function AgentOfficeIsometric({
  participants,
  selectedId,
  onSelect,
}: {
  participants: ParticipantRecord[];
  selectedId: string | null;
  onSelect: (participantId: string) => void;
}) {
  const { t } = useI18n();
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const avatarRefs = useRef(new Map<string, HTMLButtonElement>());
  const [viewport, setViewport] = useState<OfficeViewport>({
    width: OFFICE_FALLBACK_VIEWPORT.width,
    height: OFFICE_FALLBACK_VIEWPORT.height,
  });
  const [manualCamera, setManualCamera] = useState<OfficeCamera | null>(null);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    camera: OfficeCamera;
  } | null>(null);
  const layout = useMemo(
    () => buildOfficeSceneLayout(participants),
    [participants],
  );
  const fitted = useMemo(
    () => fitCamera(layout.width, layout.height, viewport),
    [layout.width, layout.height, viewport],
  );
  const camera = useMemo(() => {
    if (!manualCamera) return fitted;
    return clampCamera(manualCamera, layout.width, layout.height, viewport);
  }, [fitted, layout.width, layout.height, manualCamera, viewport]);

  const applyCamera = useCallback(
    (next: OfficeCamera) => {
      setManualCamera(clampCamera(next, layout.width, layout.height, viewport));
    },
    [layout.width, layout.height, viewport],
  );

  function resetCamera() {
    setManualCamera(null);
  }

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    function measure() {
      const rect = element?.getBoundingClientRect();
      if (!rect || rect.width < 1 || rect.height < 1) return;
      const width = Math.round(rect.width);
      const height = Math.round(rect.height);
      setViewport((current) =>
        current.width === width && current.height === height
          ? current
          : { width, height },
      );
    }
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    function onWheel(event: WheelEvent) {
      event.preventDefault();
      const rect = element?.getBoundingClientRect();
      const focusX = event.clientX - (rect?.left ?? 0);
      const focusY = event.clientY - (rect?.top ?? 0);
      const delta = event.deltaY * (event.deltaMode === 1 ? 16 : 1);
      applyCamera(
        zoomCamera(
          camera,
          Math.exp(-delta * 0.0015),
          focusX,
          focusY,
          layout.width,
          layout.height,
          viewport,
        ),
      );
    }
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [applyCamera, camera, layout.width, layout.height, viewport]);

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const target = event.target instanceof Element ? event.target : null;
    if (
      target?.closest("button,a,input,select,textarea,[data-office-no-drag]")
    ) {
      return;
    }
    drag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      camera,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDragging(true);
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    applyCamera(
      panCamera(
        current.camera,
        event.clientX - current.startX,
        event.clientY - current.startY,
        layout.width,
        layout.height,
        viewport,
      ),
    );
  }

  function endDrag(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    drag.current = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    setDragging(false);
  }

  function registerRef(
    participantId: string,
    element: HTMLButtonElement | null,
  ) {
    if (element) avatarRefs.current.set(participantId, element);
    else avatarRefs.current.delete(participantId);
  }

  const zoom = zoomPercent(camera);
  const labelDetail = camera.zoom >= 0.7;
  const labelScale = Math.min(1.6, Math.max(1, 1 / camera.zoom));
  const canZoomIn = camera.zoom < OFFICE_MAX_ZOOM;
  const canZoomOut = camera.zoom > OFFICE_MIN_ZOOM;
  const canPan = (deltaX: number, deltaY: number) => {
    const next = panCamera(
      camera,
      deltaX,
      deltaY,
      layout.width,
      layout.height,
      viewport,
    );
    return next.x !== camera.x || next.y !== camera.y;
  };
  const centerX = viewport.width / 2;
  const centerY = viewport.height / 2;

  if (participants.length === 0) {
    return (
      <Panel className="office-scene-empty" data-testid="office-scene-empty">
        <EmptyState
          title={t("agentOffice.emptyTitle")}
          description={t("agentOffice.emptyDescription")}
        />
      </Panel>
    );
  }

  return (
    <section
      className={cn("office-scene", "agent-floor", dragging && "is-dragging")}
      aria-label={t("agentOffice.scene.label")}
      data-label-detail={labelDetail ? "true" : "false"}
      data-testid="agent-floor"
    >
      <div className="office-scene-toolbar">
        <fieldset className="office-scene-controls">
          <legend className="ui-visually-hidden">
            {t("agentOffice.scene.pan")}
          </legend>
          <IconButton
            label={t("agentOffice.scene.panLeft")}
            disabled={!canPan(-OFFICE_PAN_STEP, 0)}
            onClick={() =>
              applyCamera(
                panCamera(
                  camera,
                  -OFFICE_PAN_STEP,
                  0,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <ArrowLeft size={16} aria-hidden="true" />
          </IconButton>
          <IconButton
            label={t("agentOffice.scene.panRight")}
            disabled={!canPan(OFFICE_PAN_STEP, 0)}
            onClick={() =>
              applyCamera(
                panCamera(
                  camera,
                  OFFICE_PAN_STEP,
                  0,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <ArrowRight size={16} aria-hidden="true" />
          </IconButton>
          <IconButton
            label={t("agentOffice.scene.panUp")}
            disabled={!canPan(0, -OFFICE_PAN_STEP)}
            onClick={() =>
              applyCamera(
                panCamera(
                  camera,
                  0,
                  -OFFICE_PAN_STEP,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <ArrowUp size={16} aria-hidden="true" />
          </IconButton>
          <IconButton
            label={t("agentOffice.scene.panDown")}
            disabled={!canPan(0, OFFICE_PAN_STEP)}
            onClick={() =>
              applyCamera(
                panCamera(
                  camera,
                  0,
                  OFFICE_PAN_STEP,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <ArrowDown size={16} aria-hidden="true" />
          </IconButton>
        </fieldset>
        <fieldset className="office-scene-controls office-scene-zoom-controls">
          <legend className="ui-visually-hidden">
            {t("agentOffice.scene.zoom")}
          </legend>
          <IconButton
            label={t("agentOffice.scene.zoomOut")}
            disabled={!canZoomOut}
            onClick={() =>
              applyCamera(
                zoomCamera(
                  camera,
                  1 / OFFICE_ZOOM_STEP,
                  centerX,
                  centerY,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <Minus size={16} aria-hidden="true" />
          </IconButton>
          <output
            className="office-scene-zoom-value"
            aria-live="polite"
            data-testid="office-zoom"
            data-zoom={camera.zoom.toFixed(3)}
          >
            {t("agentOffice.scene.zoomValue", { percent: zoom })}
          </output>
          <IconButton
            label={t("agentOffice.scene.zoomIn")}
            disabled={!canZoomIn}
            onClick={() =>
              applyCamera(
                zoomCamera(
                  camera,
                  OFFICE_ZOOM_STEP,
                  centerX,
                  centerY,
                  layout.width,
                  layout.height,
                  viewport,
                ),
              )
            }
          >
            <Plus size={16} aria-hidden="true" />
          </IconButton>
          <Button
            size="sm"
            variant="quiet"
            onClick={resetCamera}
            title={t("agentOffice.scene.fit")}
          >
            <Maximize2 size={14} aria-hidden="true" />
            {t("agentOffice.scene.fit")}
          </Button>
        </fieldset>
      </div>

      <div
        className="office-scene-viewport"
        ref={viewportRef}
        data-testid="office-scene-viewport"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <div
          className="office-scene-world"
          data-testid="office-scene-world"
          data-zoom={camera.zoom.toFixed(3)}
          style={
            {
              width: layout.width,
              height: layout.height,
              ...cameraStyle(camera),
              "--office-label-scale": labelScale.toFixed(2),
            } as CSSProperties
          }
        >
          {layout.zones.map((zone) => (
            <OfficeZoneStation
              key={zone.role}
              zone={zone}
              selectedId={selectedId}
              onSelect={onSelect}
              slots={layout.slots}
              refs={avatarRefs.current}
              registerRef={registerRef}
            />
          ))}
        </div>
      </div>

      {layout.duplicateIds.length > 0 && (
        <p className="callout callout-attention" role="status">
          {t(
            layout.duplicateIds.length === 1
              ? "agentOffice.scene.duplicatesOne"
              : "agentOffice.scene.duplicatesOther",
            { count: layout.duplicateIds.length },
          )}
        </p>
      )}
      <p className="muted office-scene-hint">{t("agentOffice.scene.hint")}</p>
    </section>
  );
}
