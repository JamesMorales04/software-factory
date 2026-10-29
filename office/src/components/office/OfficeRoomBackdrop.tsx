import { RoomFloorFurniture } from "./OfficeRoomFurniture";
import { OFFICE_ZONE_HEADER, type OfficeZone } from "./officeSceneLayout";

interface Point {
  x: number;
  y: number;
}

function clippedRect(
  left: number,
  top: number,
  right: number,
  bottom: number,
  cut: number,
): Point[] {
  return [
    { x: left + cut, y: top },
    { x: right - cut, y: top },
    { x: right, y: top + cut },
    { x: right, y: bottom - cut },
    { x: right - cut, y: bottom },
    { x: left + cut, y: bottom },
    { x: left, y: bottom - cut },
    { x: left, y: top + cut },
  ];
}

function points(pointsList: Point[]): string {
  return pointsList.map((point) => `${point.x},${point.y}`).join(" ");
}

function offset(pointsList: Point[], dx: number, dy: number): Point[] {
  return pointsList.map((point) => ({
    x: point.x + dx,
    y: point.y + dy,
  }));
}

function WallClock({ x, y }: { x: number; y: number }) {
  return (
    <g className="office-wall-clock" transform={`translate(${x} ${y})`}>
      <circle className="office-wall-clock-face" cx="0" cy="0" r="10" />
      <circle className="office-wall-clock-inner" cx="0" cy="0" r="7.6" />
      <path className="office-wall-clock-hours" d="M0 -5 V0 L4 2" />
      <circle className="office-wall-clock-pin" cx="0" cy="0" r="1.1" />
    </g>
  );
}

function Whiteboard({ x, y, width }: { x: number; y: number; width: number }) {
  return (
    <g className="office-whiteboard" transform={`translate(${x} ${y})`}>
      <rect
        className="office-whiteboard-frame"
        x="0"
        y="0"
        width={width}
        height="30"
        rx="2"
      />
      <rect
        className="office-whiteboard-surface"
        x="2.5"
        y="2.5"
        width={width - 5}
        height="25"
        rx="1"
      />
      <rect
        className="office-whiteboard-note"
        x="8"
        y="6"
        width="12"
        height="8"
        rx="1"
      />
      <rect
        className="office-whiteboard-note office-whiteboard-note-b"
        x="23"
        y="7"
        width="10"
        height="7"
        rx="1"
      />
      <rect
        className="office-whiteboard-note office-whiteboard-note-c"
        x={width - 24}
        y="5"
        width="11"
        height="8"
        rx="1"
      />
      <path
        className="office-whiteboard-link"
        d={`M14 14 L20 20 L${Math.max(30, width - 28)} 16`}
      />
      <path
        className="office-whiteboard-line"
        d={`M8 22 H${Math.max(34, width - 34)} M8 26 H${Math.max(30, width - 52)}`}
      />
      <circle className="office-whiteboard-node" cx="20" cy="20" r="2.4" />
      <circle
        className="office-whiteboard-node office-whiteboard-node-b"
        cx={Math.max(30, width - 28)}
        cy="16"
        r="2.4"
      />
    </g>
  );
}

function Pinboard({ x, y, width }: { x: number; y: number; width: number }) {
  return (
    <g className="office-pinboard" transform={`translate(${x} ${y})`}>
      <rect
        className="office-pinboard-frame"
        x="0"
        y="0"
        width={width}
        height="28"
        rx="2"
      />
      <rect
        className="office-pinboard-cork"
        x="2.5"
        y="2.5"
        width={width - 5}
        height="23"
        rx="1"
      />
      <rect
        className="office-pinboard-note office-pinboard-note-a"
        x="7"
        y="6"
        width="11"
        height="8"
        rx="1"
      />
      <rect
        className="office-pinboard-note office-pinboard-note-b"
        x="21"
        y="8"
        width="9"
        height="7"
        rx="1"
      />
      <rect
        className="office-pinboard-note office-pinboard-note-c"
        x={width - 25}
        y="6"
        width="10"
        height="8"
        rx="1"
      />
      <path
        className="office-pinboard-string"
        d={`M12 14 Q${width / 2} 20 ${width - 20} 12`}
      />
      <circle className="office-pinboard-pin" cx="7" cy="6" r="1" />
      <circle className="office-pinboard-pin" cx="21" cy="8" r="1" />
      <circle className="office-pinboard-pin" cx={width - 25} cy="6" r="1" />
    </g>
  );
}

function ToolBoard({ x, y, width }: { x: number; y: number; width: number }) {
  return (
    <g className="office-toolboard" transform={`translate(${x} ${y})`}>
      <rect
        className="office-toolboard-frame"
        x="0"
        y="0"
        width={width}
        height="26"
        rx="2"
      />
      <rect
        className="office-toolboard-face"
        x="2"
        y="2"
        width={width - 4}
        height="22"
        rx="1"
      />
      <path
        className="office-toolboard-hook"
        d="M8 6 V10 M8 10 q-2 2 0 3 M20 5 V9 M20 9 q-2 2 0 3 M32 6 V10 M32 10 q-2 2 0 3"
      />
      <rect
        className="office-toolboard-tool"
        x="6"
        y="12"
        width="4"
        height="9"
        rx="1"
      />
      <rect
        className="office-toolboard-tool office-toolboard-tool-b"
        x="18"
        y="11"
        width="5"
        height="10"
        rx="1"
      />
      <path
        className="office-toolboard-wrench"
        d="M29 13 q4 -3 6 1 l-3 2 z M32 16 l3 4"
      />
      <circle
        className="office-toolboard-gauge"
        cx={width - 18}
        cy="12"
        r="5.5"
      />
      <path
        className="office-toolboard-gauge-line"
        d={`M${width - 18} 12 l2 -3`}
      />
    </g>
  );
}

function WallShelf({ x, y, width }: { x: number; y: number; width: number }) {
  return (
    <g className="office-wall-shelf" transform={`translate(${x} ${y})`}>
      <rect
        className="office-wall-shelf-board"
        x="0"
        y="0"
        width={width}
        height="3"
        rx="1"
      />
      <path
        className="office-wall-shelf-bracket"
        d={`M4 3 V8 M${width - 4} 3 V8`}
      />
      <rect
        className="office-wall-shelf-book office-book-a"
        x="5"
        y="-9"
        width="4"
        height="9"
        rx="0.6"
      />
      <rect
        className="office-wall-shelf-book office-book-c"
        x="10"
        y="-7"
        width="4"
        height="7"
        rx="0.6"
      />
      <rect
        className="office-wall-shelf-book office-book-d"
        x="15"
        y="-10"
        width="4"
        height="10"
        rx="0.6"
      />
      <circle className="office-wall-shelf-orb" cx={width - 12} cy="-3" r="3" />
    </g>
  );
}

function RoomWindow({
  x,
  width,
  glowId,
}: {
  x: number;
  width: number;
  glowId: string;
}) {
  return (
    <g className="office-room-window" transform={`translate(${x} 10)`}>
      <ellipse
        className="office-room-window-glow"
        cx={width / 2}
        cy="14"
        rx={width * 0.8}
        ry="26"
        fill={`url(#${glowId})`}
      />
      <rect
        className="office-room-window-frame"
        x="0"
        y="0"
        width={width}
        height="30"
        rx="2"
      />
      <rect
        className="office-room-window-glass"
        x="3"
        y="3"
        width={width - 6}
        height="24"
        rx="1"
      />
      <path
        className="office-room-window-city"
        d={`M5 24 V18 H12 V15 H19 V21 H26 V17 H33 V23 H40 V14 H${width - 4} V24 Z`}
      />
      <path
        className="office-room-window-stars"
        d="M9 7 h1 M22 9 h1 M38 6 h1"
      />
      <path className="office-room-window-mullion" d={`M${width / 2} 3 V27`} />
    </g>
  );
}

export function OfficeRoomBackdrop({ zone }: { zone: OfficeZone }) {
  const width = zone.width;
  const height = zone.height;
  const glowId = `office-room-glow-${zone.id}`;
  const floor = clippedRect(
    10,
    OFFICE_ZONE_HEADER - 5,
    width - 10,
    height - 8,
    16,
  );
  const rug = clippedRect(
    24,
    OFFICE_ZONE_HEADER + 15,
    width - 24,
    height - 24,
    12,
  );
  const wallWidth = Math.max(0, width - 20);
  const wall = [
    { x: 10, y: 4 },
    { x: width - 10, y: 4 },
    { x: width - 10, y: OFFICE_ZONE_HEADER - 6 },
    { x: 10, y: OFFICE_ZONE_HEADER - 6 },
  ];
  const hasRoomForWallArt = wallWidth >= 380;
  const hasRoomForFullWallDecor = wallWidth >= 460;
  const wallArtWidth = Math.min(118, wallWidth - 170);

  return (
    <svg
      className="office-room"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <radialGradient id={glowId} cx="50%" cy="50%" r="50%">
          <stop offset="0" stopColor="var(--office-lamp)" stopOpacity="0.42" />
          <stop
            offset="0.55"
            stopColor="var(--office-lamp)"
            stopOpacity="0.13"
          />
          <stop offset="1" stopColor="var(--office-lamp)" stopOpacity="0" />
        </radialGradient>
        <pattern
          id={`office-room-planks-${zone.id}`}
          width="72"
          height="36"
          patternUnits="userSpaceOnUse"
          patternTransform="rotate(23)"
        >
          <rect width="72" height="36" fill="transparent" />
          <path className="office-room-plank-line" d="M0 1 H72 M0 18 H72" />
          <path className="office-room-plank-seam" d="M18 0 V18 M54 18 V36" />
        </pattern>
      </defs>
      <polygon
        className="office-room-floor-side"
        points={points(offset(floor, 0, 9))}
      />
      <polygon className="office-room-floor" points={points(floor)} />
      <polygon
        className="office-room-floor-planks"
        points={points(floor)}
        fill={`url(#office-room-planks-${zone.id})`}
      />
      <polygon className="office-room-rug" points={points(rug)} />
      <polygon
        className="office-room-rug-inner"
        points={points(offset(rug, 6, 5))}
      />
      <polygon
        className="office-room-floor-shine"
        points={points(offset(floor, 5, 4))}
      />
      <ellipse
        className="office-room-light"
        cx={width / 2}
        cy={height * 0.6}
        rx={width * 0.48}
        ry={height * 0.38}
        fill={`url(#${glowId})`}
      />
      <g className="office-room-wall">
        <polygon
          className="office-room-wall-return office-room-wall-return-left"
          points={`10,4 10,${OFFICE_ZONE_HEADER - 6} 2,${OFFICE_ZONE_HEADER} 2,10`}
        />
        <polygon
          className="office-room-wall-return office-room-wall-return-right"
          points={`${width - 10},4 ${width - 10},${OFFICE_ZONE_HEADER - 6} ${width - 2},${OFFICE_ZONE_HEADER} ${width - 2},10`}
        />
        <polygon
          className="office-room-wall-cap"
          points={points(offset(wall, 0, -4))}
        />
        <polygon className="office-room-wall-face" points={points(wall)} />
        <path
          className="office-room-wall-panel"
          d={`M20 8 V${OFFICE_ZONE_HEADER - 8} M${width - 20} 8 V${OFFICE_ZONE_HEADER - 8}`}
        />
        {Array.from(
          { length: Math.max(0, Math.floor((wallWidth - 40) / 44)) },
          (_, index) => 36 + index * 44,
        ).map((seamX) => (
          <path
            key={`wall-seam-${seamX}`}
            className="office-room-wall-seam"
            d={`M${seamX} 8 V${OFFICE_ZONE_HEADER - 9}`}
          />
        ))}
        <path
          className="office-room-wall-trim"
          d={`M10 ${OFFICE_ZONE_HEADER - 6} H${width - 10}`}
        />
        <path
          className="office-room-skirting"
          d={`M10 ${OFFICE_ZONE_HEADER - 3} H${width - 10}`}
        />
        {wallWidth > 430 && (
          <RoomWindow
            x={wallWidth > 560 ? width * 0.42 : 210}
            width={84}
            glowId={glowId}
          />
        )}
        {hasRoomForWallArt && zone.role === "LEAD" && (
          <Whiteboard x={width - 146} y={12} width={wallArtWidth} />
        )}
        {hasRoomForWallArt &&
          (zone.role === "IMPLEMENTER" || zone.role === "REVIEWER") && (
            <Pinboard x={width - 146} y={12} width={wallArtWidth} />
          )}
        {hasRoomForWallArt &&
          (zone.role === "VERIFIER" || zone.role === "WORKER") && (
            <ToolBoard x={width - 146} y={13} width={wallArtWidth} />
          )}
        {hasRoomForFullWallDecor && (
          <WallShelf
            x={width - 206}
            y={44}
            width={Math.min(46, wallArtWidth - 20)}
          />
        )}
        {(hasRoomForFullWallDecor || !hasRoomForWallArt) && wallWidth > 320 && (
          <WallClock
            x={hasRoomForFullWallDecor ? width - 174 : width - 50}
            y={27}
          />
        )}
        <g
          className="office-wall-sconce"
          transform={`translate(${width - 12} 30)`}
        >
          <path className="office-room-sconce" d="M-3 0 H3 L2 8 H-2 Z" />
          <circle className="office-room-sconce-bulb" cx="0" cy="9" r="2.6" />
        </g>
        <g className="office-wall-sconce" transform="translate(12 30)">
          <path className="office-room-sconce" d="M-3 0 H3 L2 8 H-2 Z" />
          <circle className="office-room-sconce-bulb" cx="0" cy="9" r="2.6" />
        </g>
      </g>
      <RoomFloorFurniture zone={zone} glowId={glowId} />
    </svg>
  );
}
