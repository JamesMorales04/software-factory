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
import { OfficeAvatar } from "./OfficeAvatar";
import { OfficeRoomBackdrop } from "./OfficeRoomBackdrop";
import { OfficeStudioShell } from "./OfficeStudioShell";
import {
  buildOfficeSceneLayout,
  cameraStyle,
  clampCamera,
  fitCamera,
  OFFICE_FALLBACK_VIEWPORT,
  OFFICE_MAX_ZOOM,
  OFFICE_MIN_ZOOM,
  OFFICE_PAN_STEP,
  OFFICE_ZOOM_STEP,
  type OfficeCamera,
  type OfficeSlot,
  type OfficeViewport,
  type OfficeZone,
  panCamera,
  zoomCamera,
  zoomPercent,
} from "./officeSceneLayout";

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
      <OfficeRoomBackdrop zone={zone} />
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
  const overlayScale = Math.min(1.4, Math.max(1, 1 / camera.zoom));
  const zoneScale = Math.min(1.35, Math.max(1, 1 / camera.zoom));
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
      className={cn(
        "office-scene",
        "office-studio",
        "agent-floor",
        dragging && "is-dragging",
      )}
      aria-label={t("agentOffice.scene.label")}
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
              "--office-overlay-scale": overlayScale.toFixed(2),
              "--office-zone-scale": zoneScale.toFixed(2),
            } as CSSProperties
          }
        >
          <OfficeStudioShell layout={layout} />
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

      {layout.emptyRoles.length > 0 && (
        <div
          className="office-scene-empty-stations"
          data-testid="office-empty-stations"
        >
          <span>{t("agentOffice.scene.emptyStations")}</span>
          <ul>
            {layout.emptyRoles.map((role) => (
              <li key={role}>{t(ZONE_META[role].labelKey)}</li>
            ))}
          </ul>
        </div>
      )}
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
