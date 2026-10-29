import type { OfficeSceneLayout, OfficeZone } from "./officeSceneLayout";
import { OFFICE_WALL_HEIGHT } from "./officeSceneLayout";

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

function polygon(items: Point[]): string {
  return items.map((point) => `${point.x},${point.y}`).join(" ");
}

function offset(items: Point[], dx: number, dy: number): Point[] {
  return items.map((point) => ({ x: point.x + dx, y: point.y + dy }));
}

function zoneByRole(
  layout: OfficeSceneLayout,
  role: OfficeZone["role"],
): OfficeZone | undefined {
  return layout.zones.find((zone) => zone.role === role);
}

function Hallway({
  x,
  y,
  width,
  height,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
}) {
  return (
    <g className="office-hallway">
      <rect
        className="office-hallway-floor"
        x={x}
        y={y}
        width={width}
        height={height}
        rx="6"
      />
      <rect
        className="office-hallway-inlay"
        x={x + 5}
        y={y + 5}
        width={Math.max(1, width - 10)}
        height={Math.max(1, height - 10)}
        rx="4"
      />
      <path
        className="office-hallway-mark"
        d={`M${x + 12} ${y + height / 2} H${x + width - 12}`}
      />
    </g>
  );
}

function StudioWindow({ x, width }: { x: number; width: number }) {
  return (
    <g className="office-studio-window" transform={`translate(${x} 7)`}>
      <rect
        className="office-studio-window-frame"
        x="0"
        y="0"
        width={width}
        height="31"
        rx="3"
      />
      <rect
        className="office-studio-window-glass"
        x="3.5"
        y="3.5"
        width={width - 7}
        height="24"
        rx="1.5"
      />
      <path
        className="office-studio-window-city"
        d={`M6 24 V18 H12 V15 H18 V21 H25 V17 H31 V23 H38 V14 H45 V20 H52 V16 H58 V24 Z`}
      />
      <path
        className="office-studio-window-stars"
        d="M8 7 h1 M19 9 h1 M34 6 h1 M47 9 h1"
      />
      <path
        className="office-studio-window-mullion"
        d={`M${width / 2} 3.5 V27.5 M3.5 15.5 H${width - 3.5}`}
      />
    </g>
  );
}

export function OfficeStudioShell({ layout }: { layout: OfficeSceneLayout }) {
  const width = layout.width;
  const height = layout.height;
  const platform = clippedRect(8, 8, width - 8, height - 8, 28);
  const planning = zoneByRole(layout, "LEAD");
  const implementation = zoneByRole(layout, "IMPLEMENTER");
  const verification = zoneByRole(layout, "VERIFIER");
  const operations = zoneByRole(layout, "WORKER");
  const review = zoneByRole(layout, "REVIEWER");
  const corridors: { x: number; y: number; width: number; height: number }[] =
    [];
  if (planning && implementation) {
    const y = implementation.y + implementation.height / 2 - 30;
    corridors.push({
      x: planning.x + planning.width - 4,
      y,
      width: implementation.x - planning.x - planning.width + 8,
      height: 60,
    });
  }
  if (implementation && verification) {
    const y = implementation.y + implementation.height / 2 - 30;
    corridors.push({
      x: implementation.x + implementation.width - 4,
      y,
      width: verification.x - implementation.x - implementation.width + 8,
      height: 60,
    });
  }
  if (planning && operations) {
    corridors.push({
      x: planning.x + planning.width / 2 - 30,
      y: planning.y + planning.height - 4,
      width: 60,
      height: operations.y - planning.y - planning.height + 8,
    });
  }
  if (verification && review) {
    corridors.push({
      x: verification.x + verification.width / 2 - 30,
      y: verification.y + verification.height - 4,
      width: 60,
      height: review.y - verification.y - verification.height + 8,
    });
  }

  const centerRoomBottom = implementation
    ? implementation.y + implementation.height
    : 0;
  const showEmblem = centerRoomBottom < height - 90;
  const windowWidth = 74;
  const windowGap = 128;
  const signCenter = width / 2;
  const windows: number[] = [];
  for (let x = 42; x + windowWidth < width - 42; x += windowGap) {
    if (x + windowWidth > signCenter - 76 && x < signCenter + 76) continue;
    windows.push(x);
  }

  return (
    <svg
      className="office-studio-shell"
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <pattern
          id="office-shell-tile"
          width="88"
          height="52"
          patternUnits="userSpaceOnUse"
          patternTransform="skewX(-24)"
        >
          <rect width="88" height="52" fill="transparent" />
          <path className="office-shell-tile-line" d="M0 0.5 H88 M0.5 0 V52" />
        </pattern>
        <radialGradient id="office-shell-warm" cx="50%" cy="50%" r="50%">
          <stop offset="0" stopColor="var(--office-lamp)" stopOpacity="0.2" />
          <stop
            offset="0.6"
            stopColor="var(--office-lamp)"
            stopOpacity="0.06"
          />
          <stop offset="1" stopColor="var(--office-lamp)" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="office-shell-floor" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#20303f" />
          <stop offset="0.55" stopColor="#172431" />
          <stop offset="1" stopColor="#101b27" />
        </linearGradient>
        <linearGradient id="office-shell-wall" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#0a1724" />
          <stop offset="1" stopColor="#102235" />
        </linearGradient>
      </defs>
      <polygon
        className="office-studio-platform-side"
        points={polygon(offset(platform, 0, 14))}
      />
      <polygon
        className="office-studio-platform-side office-studio-platform-side-right"
        points={polygon(offset(platform, 8, 8))}
      />
      <polygon
        className="office-studio-platform"
        points={polygon(platform)}
        fill="url(#office-shell-floor)"
      />
      <polygon
        className="office-studio-platform-tile"
        points={polygon(platform)}
        fill="url(#office-shell-tile)"
      />
      <g className="office-studio-wall" transform={`translate(0 0)`}>
        <rect
          className="office-studio-wall-face"
          x="8"
          y="0"
          width={width - 16}
          height={OFFICE_WALL_HEIGHT}
          fill="url(#office-shell-wall)"
        />
        <path
          className="office-studio-wall-panel"
          d={`M20 4 V${OFFICE_WALL_HEIGHT - 2} M${width - 20} 4 V${OFFICE_WALL_HEIGHT - 2}`}
        />
        {Array.from(
          { length: Math.max(0, Math.floor((width - 120) / 96)) },
          (_, index) => 104 + index * 96,
        ).map((panelX) => (
          <path
            key={`panel-${panelX}`}
            className="office-studio-wall-seam"
            d={`M${panelX} 5 V${OFFICE_WALL_HEIGHT - 3}`}
          />
        ))}
        {windows.map((x) => (
          <StudioWindow key={`window-${x}`} x={x} width={windowWidth} />
        ))}
        <path
          className="office-studio-wall-trim"
          d={`M8 ${OFFICE_WALL_HEIGHT} H${width - 8}`}
        />
        <g
          className="office-studio-sign"
          transform={`translate(${width / 2 - 58} 10)`}
        >
          <rect x="0" y="0" width="116" height="30" rx="4" />
          <path d="M12 9 h16 v4 h-11 v3 h9 v4 h-9 v5 h-5 z" />
          <path
            className="office-studio-sign-mark"
            d="M40 10 h34 M40 15 h22 M40 20 h30"
          />
          <circle className="office-studio-sign-lamp" cx="92" cy="15" r="3" />
        </g>
      </g>
      {corridors.map((corridor) => (
        <Hallway
          key={`hall-${corridor.x}-${corridor.y}`}
          x={corridor.x}
          y={corridor.y}
          width={corridor.width}
          height={corridor.height}
        />
      ))}
      <ellipse
        className="office-studio-ambient"
        cx={width * 0.3}
        cy={height * 0.42}
        rx={width * 0.3}
        ry={height * 0.3}
        fill="url(#office-shell-warm)"
      />
      <ellipse
        className="office-studio-ambient"
        cx={width * 0.72}
        cy={height * 0.62}
        rx={width * 0.28}
        ry={height * 0.28}
        fill="url(#office-shell-warm)"
      />
      {showEmblem && (
        <g
          className="office-studio-emblem"
          transform={`translate(${width / 2} ${height - 58})`}
        >
          <polygon
            className="office-studio-emblem-ring"
            points="0,-34 29,-17 29,17 0,34 -29,17 -29,-17"
          />
          <polygon
            className="office-studio-emblem-inner"
            points="0,-23 20,-11 20,11 0,23 -20,11 -20,-11"
          />
          <circle className="office-studio-emblem-core" cx="0" cy="0" r="4" />
          <path
            className="office-studio-emblem-tick"
            d="M0 -31 V-25 M0 25 V31 M-26 -14 L-21 -11 M26 -14 L21 -11"
          />
        </g>
      )}
    </svg>
  );
}
