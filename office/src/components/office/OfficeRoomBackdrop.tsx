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

function Plant({ x, y, scale }: { x: number; y: number; scale: number }) {
  return (
    <g
      className="office-plant"
      transform={`translate(${x} ${y}) scale(${scale})`}
    >
      <ellipse className="office-plant-shadow" cx="0" cy="18" rx="13" ry="3" />
      <path className="office-plant-pot" d="M-8 4 H8 L6 17 H-6 Z" />
      <path className="office-plant-pot-rim" d="M-9 3 H9 V6 H-9 Z" />
      <path
        className="office-plant-stem"
        d="M0 4 V-10 M0 -2 Q-8 -8 -10 -17 M0 -1 Q8 -7 10 -16"
      />
      <ellipse
        className="office-plant-leaf"
        cx="-13"
        cy="-18"
        rx="6"
        ry="3.4"
        transform="rotate(24 -13 -18)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="-6"
        cy="-24"
        rx="6.4"
        ry="3.2"
        transform="rotate(-18 -6 -24)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="4"
        cy="-25"
        rx="6.4"
        ry="3.2"
        transform="rotate(16 4 -25)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="12"
        cy="-16"
        rx="5.8"
        ry="3.2"
        transform="rotate(-26 12 -16)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="1"
        cy="-11"
        rx="5.4"
        ry="3"
        transform="rotate(8 1 -11)"
      />
    </g>
  );
}

function Lamp({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-floor-lamp" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="6"
        rx="44"
        ry="30"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="12"
        rx="10"
        ry="2.6"
      />
      <rect
        className="office-lamp-pole"
        x="-1"
        y="-18"
        width="2"
        height="30"
        rx="1"
      />
      <path className="office-lamp-shade" d="M-9 -25 H9 L6 -15 H-6 Z" />
      <ellipse className="office-lamp-light" cx="0" cy="-14" rx="5" ry="1.8" />
    </g>
  );
}

function WallBoard({ x, y, width }: { x: number; y: number; width: number }) {
  return (
    <g className="office-room-board" transform={`translate(${x} ${y})`}>
      <rect
        className="office-room-board-frame"
        x="0"
        y="0"
        width={width}
        height="26"
        rx="2"
      />
      <rect
        className="office-room-board-surface"
        x="3"
        y="3"
        width={width - 6}
        height="20"
        rx="1"
      />
      <rect
        className="office-board-note"
        x="7"
        y="6"
        width="10"
        height="8"
        rx="1"
      />
      <rect
        className="office-board-note"
        x="20"
        y="8"
        width="9"
        height="7"
        rx="1"
      />
      <path className="office-room-board-line" d="M34 9 H44 M34 13 H41" />
      {width > 74 && (
        <>
          <rect
            className="office-board-note"
            x="50"
            y="6"
            width="9"
            height="7"
            rx="1"
          />
          <path className="office-room-board-line" d="M34 18 H62" />
        </>
      )}
    </g>
  );
}

export function OfficeRoomBackdrop({ zone }: { zone: OfficeZone }) {
  const width = zone.width;
  const height = zone.height;
  const glowId = `office-room-glow-${zone.id}`;
  const floor = clippedRect(
    8,
    OFFICE_ZONE_HEADER - 4,
    width - 8,
    height - 6,
    18,
  );
  const wall = [
    { x: 8, y: 4 },
    { x: width - 8, y: 4 },
    { x: width - 14, y: 12 },
    { x: width - 14, y: OFFICE_ZONE_HEADER + 2 },
    { x: 14, y: OFFICE_ZONE_HEADER + 2 },
    { x: 14, y: 12 },
  ];
  const rug = clippedRect(
    28,
    OFFICE_ZONE_HEADER + 20,
    width - 28,
    height - 26,
    12,
  );
  const hasRoomForBoard = width >= 300;

  return (
    <svg
      className="office-room"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <radialGradient id={glowId} cx="50%" cy="50%" r="50%">
          <stop offset="0" stopColor="var(--office-lamp)" stopOpacity="0.48" />
          <stop
            offset="0.55"
            stopColor="var(--office-lamp)"
            stopOpacity="0.16"
          />
          <stop offset="1" stopColor="var(--office-lamp)" stopOpacity="0" />
        </radialGradient>
      </defs>
      <polygon
        className="office-room-floor-side"
        points={points(offset(floor, 0, 9))}
      />
      <polygon className="office-room-floor" points={points(floor)} />
      <polygon className="office-room-rug" points={points(rug)} />
      <polygon
        className="office-room-floor-shine"
        points={points(offset(floor, 4, 5))}
      />
      <polygon
        className="office-room-floor-side"
        points={points(offset(floor, 0, 9))}
      />
      <polygon className="office-room-floor" points={points(floor)} />
      <polygon className="office-room-rug" points={points(rug)} />
      <polygon
        className="office-room-floor-shine"
        points={points(offset(floor, 4, 5))}
      />
      <ellipse
        className="office-room-light"
        cx={width / 2}
        cy={height * 0.58}
        rx={width * 0.5}
        ry={height * 0.42}
        fill={`url(#${glowId})`}
      />
      <g className="office-room-wall">
        <polygon className="office-room-wall-face" points={points(wall)} />
        <path className="office-room-wall-trim" d={`M 8 4 H ${width - 8}`} />
        <path
          className="office-room-wall-seam"
          d={`M 14 12 V ${OFFICE_ZONE_HEADER + 2} M ${width - 14} 12 V ${OFFICE_ZONE_HEADER + 2}`}
        />
      </g>
      {hasRoomForBoard && (
        <WallBoard x={width - 118} y={13} width={Math.min(80, width - 140)} />
      )}
      <Lamp x={width - 38} y={height - 56} glowId={glowId} />
      <Plant x={26} y={height - 18} scale={0.9} />
      {width > 300 && <Plant x={46} y={height - 14} scale={0.62} />}
    </svg>
  );
}
