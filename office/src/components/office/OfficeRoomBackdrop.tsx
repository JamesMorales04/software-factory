import { RoomFloorFurniture } from "./OfficeRoomFurniture";
import { OFFICE_ZONE_HEADER, type OfficeZone } from "./officeSceneLayout";

interface Point {
  x: number;
  y: number;
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

function StudioPartition({ zone }: { zone: OfficeZone }) {
  const width = zone.width;
  const alignRight = zone.role === "VERIFIER" || zone.role === "REVIEWER";
  const panelWidth = Math.min(340, Math.max(164, width - 60));
  const left = alignRight ? width - panelWidth - 16 : 16;
  const top = 3;
  const height = 76;
  const depth = 6;
  const boardWidth = Math.min(210, panelWidth - 84);
  const hasBoard = panelWidth >= 196;
  const boardLeft = left + (panelWidth - boardWidth) / 2 + 8;
  const boardY = top + 34;
  const role = zone.role;

  return (
    <g className="office-partition" data-role={role}>
      <polygon
        className="office-partition-shadow"
        points={points([
          { x: left + 10, y: top + height + 4 },
          { x: left + panelWidth + 16, y: top + height - 4 },
          { x: left + panelWidth + 16, y: top + height + 6 },
          { x: left + 10, y: top + height + 14 },
        ])}
      />
      <rect
        className="office-partition-post"
        x={left}
        y={top - 2}
        width="5"
        height={height + 6}
        rx="1.5"
      />
      <rect
        className="office-partition-post"
        x={left + panelWidth - 5}
        y={top - 2}
        width="5"
        height={height + 6}
        rx="1.5"
      />
      <polygon
        className="office-partition-side"
        points={points([
          { x: left + panelWidth, y: top },
          { x: left + panelWidth + depth, y: top - depth },
          { x: left + panelWidth + depth, y: top + height - depth },
          { x: left + panelWidth, y: top + height },
        ])}
      />
      <rect
        className="office-partition-face"
        x={left}
        y={top}
        width={panelWidth}
        height={height}
        rx="2"
      />
      <rect
        className="office-partition-inset"
        x={left + 5}
        y={top + 5}
        width={panelWidth - 10}
        height={height - 10}
        rx="1.5"
      />
      <path
        className="office-partition-accent"
        d={`M${left + 8} ${top + 5} H${left + panelWidth - 8}`}
      />
      <path
        className="office-partition-seam"
        d={`M${left + 12} ${top + 8} V${top + height - 8} M${left + panelWidth - 12} ${top + 8} V${top + height - 8}`}
      />
      {hasBoard &&
        (role === "LEAD" ? (
          <Whiteboard x={boardLeft} y={boardY} width={boardWidth} />
        ) : role === "VERIFIER" || role === "WORKER" ? (
          <ToolBoard x={boardLeft} y={boardY + 2} width={boardWidth} />
        ) : (
          <Pinboard x={boardLeft} y={boardY + 2} width={boardWidth} />
        ))}
      <g
        className="office-partition-sconce"
        transform={`translate(${left + panelWidth - 14} ${top + 12})`}
      >
        <path className="office-room-sconce" d="M-3 0 H3 L2 8 H-2 Z" />
        <circle className="office-room-sconce-bulb" cx="0" cy="9" r="2.4" />
      </g>
      <g
        className="office-partition-sconce"
        transform={`translate(${left + 14} ${top + 12})`}
      >
        <path className="office-room-sconce" d="M-3 0 H3 L2 8 H-2 Z" />
        <circle className="office-room-sconce-bulb" cx="0" cy="9" r="2.4" />
      </g>
      <path
        className="office-partition-base"
        d={`M${left - 2} ${top + height} H${left + panelWidth + 2}`}
      />
    </g>
  );
}

export function OfficeRoomBackdrop({ zone }: { zone: OfficeZone }) {
  const width = zone.width;
  const height = zone.height;
  const glowId = `office-room-glow-${zone.id}`;
  const rugTop = OFFICE_ZONE_HEADER - 8;
  const rugBottom = height - 7;
  const rug = [
    { x: 18, y: rugTop + 3 },
    { x: width - 8, y: rugTop },
    { x: width - 18, y: rugBottom },
    { x: 6, y: rugBottom - 3 },
  ];
  const rugInner = offset(
    [
      { x: 27, y: rugTop + 9 },
      { x: width - 19, y: rugTop + 6 },
      { x: width - 27, y: rugBottom - 9 },
      { x: 15, y: rugBottom - 12 },
    ],
    0,
    0,
  );

  return (
    <svg
      className="office-room"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <radialGradient id={glowId} cx="50%" cy="50%" r="50%">
          <stop offset="0" stopColor="var(--office-lamp)" stopOpacity="0.36" />
          <stop
            offset="0.55"
            stopColor="var(--office-lamp)"
            stopOpacity="0.12"
          />
          <stop offset="1" stopColor="var(--office-lamp)" stopOpacity="0" />
        </radialGradient>
      </defs>
      <polygon
        className="office-room-rug-shadow"
        points={points(offset(rug, 0, 7))}
      />
      <polygon className="office-room-rug" points={points(rug)} />
      <polygon className="office-room-rug-inner" points={points(rugInner)} />
      <path
        className="office-room-rug-stitch"
        d={`M${rugInner[0].x + 4} ${rugInner[0].y + 6} L${rugInner[3].x + 4} ${rugInner[3].y - 6}`}
      />
      <ellipse
        className="office-room-light"
        cx={width / 2}
        cy={height * 0.64}
        rx={width * 0.46}
        ry={height * 0.34}
        fill={`url(#${glowId})`}
      />
      <StudioPartition zone={zone} />
      <RoomFloorFurniture zone={zone} glowId={glowId} />
    </svg>
  );
}
