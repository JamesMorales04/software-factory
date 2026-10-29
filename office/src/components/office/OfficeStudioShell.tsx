import { Plant } from "./OfficeRoomFurniture";
import type { OfficeSceneLayout, OfficeZone } from "./officeSceneLayout";
import {
  OFFICE_TOP_PADDING,
  OFFICE_WALL_HEIGHT,
  OFFICE_WORLD_PADDING,
} from "./officeSceneLayout";

interface Point {
  x: number;
  y: number;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
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

/** A carpet runner aligned with the 2:1 studio floor grid. */
function Runner({
  x,
  y,
  width,
  height,
  orientation,
  tone,
}: Rect & { orientation: "horizontal" | "vertical"; tone: string }) {
  const skew = orientation === "vertical" ? 7 : 5;
  const shape =
    orientation === "vertical"
      ? [
          { x: x + skew, y },
          { x: x + width + skew, y },
          { x: x + width, y: y + height },
          { x, y: y + height },
        ]
      : [
          { x, y: y + skew },
          { x: x + width, y },
          { x: x + width, y: y + height },
          { x, y: y + height + skew },
        ];
  const inner = offset(shape, 0, 0).map((point) => ({
    x: point.x + (orientation === "vertical" ? 3 : 3),
    y: point.y + 3,
  }));
  return (
    <g
      className="office-runner office-hallway"
      style={{ "--runner-tone": tone } as React.CSSProperties}
    >
      <polygon className="office-runner-base" points={polygon(shape)} />
      <polygon className="office-runner-inlay" points={polygon(inner)} />
      <path
        className="office-runner-mark"
        d={
          orientation === "vertical"
            ? `M${x + width / 2 + skew / 2} ${y + 12} V${y + height - 12}`
            : `M${x + 12} ${y + height / 2 + skew / 2} H${x + width - 12}`
        }
      />
    </g>
  );
}

function StudioWindow({ x, width }: { x: number; width: number }) {
  const height = 37;
  return (
    <g className="office-studio-window" transform={`translate(${x} 9)`}>
      <rect
        className="office-studio-window-glow"
        x="-10"
        y="-3"
        width={width + 20}
        height={height + 12}
        rx="4"
      />
      <rect
        className="office-studio-window-frame"
        x="0"
        y="0"
        width={width}
        height={height}
        rx="2"
      />
      <rect
        className="office-studio-window-glass"
        x="3.5"
        y="3.5"
        width={width - 7}
        height={height - 7}
        rx="1"
        fill="url(#office-window-glass)"
      />
      <path
        className="office-studio-window-lights"
        d={`M6 ${height - 14} h2 M13 ${height - 20} h2 M22 ${height - 12} h2 M${width - 24} ${height - 17} h2 M${width - 15} ${height - 12} h2`}
      />
      <path
        className="office-studio-window-city"
        d={`M5 ${height - 10} V${height - 18} H${width * 0.16} V${height - 24} H${width * 0.3} V${height - 14} H${width * 0.45} V${height - 27} H${width * 0.58} V${height - 19} H${width * 0.74} V${height - 23} H${width - 5} V${height - 10} Z`}
      />
      <path
        className="office-studio-window-stars"
        d="M8 8 h1 M17 12 h1 M31 7 h1 M45 11 h1 M58 8 h1 M71 12 h1"
      />
      <path
        className="office-studio-window-mullion"
        d={`M${width / 2} 3.5 V${height - 3.5} M3.5 ${height * 0.55} H${width - 3.5}`}
      />
    </g>
  );
}

function FloorEmblem({ x, y }: { x: number; y: number }) {
  return (
    <g className="office-studio-emblem" transform={`translate(${x} ${y})`}>
      <polygon
        className="office-studio-emblem-ring"
        points="0,-30 26,-15 26,15 0,30 -26,15 -26,-15"
      />
      <polygon
        className="office-studio-emblem-inner"
        points="0,-19 16,-9 16,9 0,19 -16,9 -16,-9"
      />
      <circle className="office-studio-emblem-core" cx="0" cy="0" r="3.4" />
    </g>
  );
}

function studioColumns(layout: OfficeSceneLayout) {
  const columns: { x: number; width: number }[] = [];
  for (const zone of layout.zones) {
    const current = columns.find((column) => column.x === zone.x);
    if (current) {
      current.width = Math.max(current.width, zone.width);
    } else {
      columns.push({ x: zone.x, width: zone.width });
    }
  }
  return columns.sort((left, right) => left.x - right.x);
}

export function OfficeStudioShell({ layout }: { layout: OfficeSceneLayout }) {
  const width = layout.width;
  const height = layout.height;
  const floorTop = layout.floorTop;
  const platform = clippedRect(8, floorTop - 10, width - 8, height - 8, 24);
  const columns = studioColumns(layout);

  const runners: (Rect & {
    orientation: "horizontal" | "vertical";
    tone: string;
  })[] = [];
  for (const column of columns) {
    const zones = layout.zones
      .filter((zone) => zone.x === column.x)
      .sort((left, right) => left.y - right.y);
    for (let index = 0; index < zones.length - 1; index += 1) {
      const top = zones[index];
      const bottom = zones[index + 1];
      runners.push({
        x: column.x + 6,
        y: top.y + top.height + 3,
        width: column.width - 12,
        height: Math.max(1, bottom.y - top.y - top.height - 6),
        orientation: "vertical",
        tone: `var(--office-zone-tone)`,
      });
    }
  }
  const topZone = layout.zones.reduce<OfficeZone | null>(
    (current, zone) => (!current || zone.y < current.y ? zone : current),
    null,
  );
  const bottomZone = layout.zones.reduce<OfficeZone | null>(
    (current, zone) =>
      !current || zone.y + zone.height > current.y + current.height
        ? zone
        : current,
    null,
  );
  if (topZone && bottomZone && bottomZone.y > topZone.y + topZone.height + 40) {
    runners.push({
      x: OFFICE_WORLD_PADDING + 8,
      y: topZone.y + topZone.height + 8,
      width: width - OFFICE_WORLD_PADDING * 2 - 16,
      height: 16,
      orientation: "horizontal",
      tone: "var(--office-zone-tone, #5f7f97)",
    });
  }

  const wallSpan = width - 24;
  const windowWidth = 88;
  const signHalf = 92;
  const windowGap = 176;
  const windows: number[] = [];
  const windowCount = Math.max(
    1,
    Math.min(7, Math.floor((wallSpan - 180) / windowGap)),
  );
  const windowStart =
    (width - ((windowCount - 1) * windowGap + windowWidth)) / 2;
  for (let index = 0; index < windowCount; index += 1) {
    const x = windowStart + index * windowGap;
    if (x + windowWidth > width / 2 - signHalf && x < width / 2 + signHalf) {
      continue;
    }
    windows.push(x);
  }

  const lowestZone = layout.zones.reduce<OfficeZone | null>(
    (current, zone) =>
      !current || zone.y + zone.height > current.y + current.height
        ? zone
        : current,
    null,
  );
  const sconceXs: number[] = [];
  for (let index = 0; index < windows.length - 1; index += 1) {
    const gapCenter =
      windows[index] +
      windowWidth +
      (windows[index + 1] - windows[index] - windowWidth) / 2;
    sconceXs.push(gapCenter);
  }
  if (windows.length > 0) {
    sconceXs.push(Math.max(34, windows[0] - 34));
    const lastWindow = windows.at(-1);
    if (lastWindow !== undefined) {
      sconceXs.push(Math.min(width - 34, lastWindow + windowWidth + 34));
    }
  }
  sconceXs.push(width / 2 - 96, width / 2 + 96);

  const showEmblem =
    (lowestZone ? lowestZone.y + lowestZone.height : floorTop) < height - 96;
  const emblemX = width / 2;
  const emblemY = height - 48;

  return (
    <svg
      className="office-studio-shell"
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="office-shell-floor" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7a5638" />
          <stop offset="0.45" stopColor="#5f422c" />
          <stop offset="1" stopColor="#3b2a1d" />
        </linearGradient>
        <linearGradient id="office-shell-wall" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#0b1929" />
          <stop offset="0.7" stopColor="#142b3e" />
          <stop offset="1" stopColor="#1d3a4f" />
        </linearGradient>
        <linearGradient id="office-wall-wash" x1="0" y1="1" x2="0" y2="0">
          <stop offset="0" stopColor="#ffbd78" stopOpacity="0.2" />
          <stop offset="1" stopColor="#ffbd78" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="office-window-glass" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#0a2234" />
          <stop offset="0.6" stopColor="#103247" />
          <stop offset="1" stopColor="#1a4a5c" />
        </linearGradient>
        <linearGradient id="office-shell-soffit" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#050d16" />
          <stop offset="1" stopColor="#0e1e2d" />
        </linearGradient>
        <radialGradient id="office-shell-warm" cx="50%" cy="50%" r="50%">
          <stop offset="0" stopColor="var(--office-lamp)" stopOpacity="0.26" />
          <stop
            offset="0.55"
            stopColor="var(--office-lamp)"
            stopOpacity="0.09"
          />
          <stop offset="1" stopColor="var(--office-lamp)" stopOpacity="0" />
        </radialGradient>
        <radialGradient id="office-shell-cool" cx="50%" cy="40%" r="60%">
          <stop offset="0" stopColor="#78c7e8" stopOpacity="0.2" />
          <stop offset="1" stopColor="#78c7e8" stopOpacity="0" />
        </radialGradient>
        <radialGradient id="office-shell-vignette" cx="50%" cy="48%" r="66%">
          <stop offset="0.45" stopColor="#02070d" stopOpacity="0" />
          <stop offset="1" stopColor="#02070d" stopOpacity="0.72" />
        </radialGradient>
        <pattern
          id="office-shell-tile"
          width="96"
          height="48"
          patternUnits="userSpaceOnUse"
        >
          <path
            className="office-shell-tile-line"
            d="M0 24 L48 0 L96 24 L48 48 Z"
          />
          <path className="office-shell-tile-seam" d="M48 0 V48" />
        </pattern>
        <pattern
          id="office-shell-planks"
          width="180"
          height="90"
          patternUnits="userSpaceOnUse"
          patternTransform="rotate(29)"
        >
          <path
            className="office-shell-plank-line"
            d="M0 0.5 H180 M0 30 H180 M0 60 H180"
          />
          <path
            className="office-shell-plank-seam"
            d="M48 0 V30 M122 30 V60 M76 60 V90"
          />
        </pattern>
      </defs>

      <polygon
        className="office-studio-platform-side"
        points={polygon(offset(platform, 0, 18))}
      />
      <polygon
        className="office-studio-platform-side office-studio-platform-side-right"
        points={polygon(offset(platform, 12, 10))}
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
      <polygon
        className="office-studio-platform-planks"
        points={polygon(platform)}
        fill="url(#office-shell-planks)"
      />
      <path
        className="office-studio-platform-edge"
        d={`M${platform[0].x + 10} ${platform[0].y + 6} H${platform[1].x - 10}`}
      />

      {runners.map((runner) => (
        <Runner
          key={`runner-${runner.orientation}-${runner.x}-${runner.y}`}
          {...runner}
        />
      ))}

      <ellipse
        className="office-studio-ambient"
        cx={width * 0.24}
        cy={floorTop + (height - floorTop) * 0.34}
        rx={width * 0.27}
        ry={(height - floorTop) * 0.34}
        fill="url(#office-shell-warm)"
      />
      <ellipse
        className="office-studio-ambient"
        cx={width * 0.76}
        cy={floorTop + (height - floorTop) * 0.6}
        rx={width * 0.28}
        ry={(height - floorTop) * 0.36}
        fill="url(#office-shell-warm)"
      />
      <ellipse
        className="office-studio-ambient office-studio-ambient-cool"
        cx={width * 0.52}
        cy={floorTop + 22}
        rx={width * 0.32}
        ry={(height - floorTop) * 0.2}
        fill="url(#office-shell-cool)"
      />

      <g className="office-studio-wall">
        <polygon
          className="office-studio-wall-return office-studio-wall-return-left"
          points={`8,0 0,10 0,${floorTop - 4} 8,${OFFICE_WALL_HEIGHT}`}
        />
        <polygon
          className="office-studio-wall-return office-studio-wall-return-right"
          points={`${width - 8},0 ${width},10 ${width},${floorTop - 4} ${width - 8},${OFFICE_WALL_HEIGHT}`}
        />
        <rect
          className="office-studio-soffit"
          x="0"
          y="0"
          width={width}
          height="10"
          fill="url(#office-shell-soffit)"
        />
        <rect
          className="office-studio-wall-face"
          x="8"
          y="10"
          width={width - 16}
          height={OFFICE_WALL_HEIGHT - 10}
          fill="url(#office-shell-wall)"
        />
        <path className="office-studio-wall-trim" d={`M8 9 H${width - 8}`} />
        <path
          className="office-studio-wall-panel"
          d={`M16 ${OFFICE_WALL_HEIGHT - 4} V12 M${width - 16} ${OFFICE_WALL_HEIGHT - 4} V12`}
        />
        {Array.from(
          { length: Math.max(0, Math.floor((width - 120) / 118)) },
          (_, index) => 82 + index * 118,
        ).map((panelX) => (
          <path
            key={`panel-${panelX}`}
            className="office-studio-wall-seam"
            d={`M${panelX} 12 V${OFFICE_WALL_HEIGHT - 4}`}
          />
        ))}
        {columns.map((column) => (
          <g key={`pilaster-${column.x}`}>
            <rect
              className="office-studio-pilaster"
              x={column.x + 2}
              y="10"
              width="6"
              height={OFFICE_WALL_HEIGHT - 12}
            />
            <rect
              className="office-studio-pilaster-light"
              x={column.x + 3}
              y={OFFICE_WALL_HEIGHT - 26}
              width="4"
              height="10"
              rx="2"
            />
          </g>
        ))}
        {windows.map((x) => (
          <StudioWindow key={`window-${x}`} x={x} width={windowWidth} />
        ))}
        <rect
          className="office-studio-wall-wash"
          x="8"
          y={OFFICE_WALL_HEIGHT - 22}
          width={width - 16}
          height="22"
          fill="url(#office-wall-wash)"
        />
        <g className="office-studio-wall-run">
          <rect
            className="office-studio-wall-run-board"
            x="26"
            y="55"
            width={width - 52}
            height="4"
            rx="1.5"
          />
          {Array.from(
            { length: Math.max(0, Math.floor((width - 100) / 92)) },
            (_, index) => 54 + index * 92,
          ).map((itemX) => (
            <g key={`wall-item-${itemX}`}>
              <rect
                className={`office-studio-wall-run-book office-book-${["a", "c", "d", "b"][itemX % 4]}`}
                x={itemX}
                y={55 - (5 + (itemX % 3) * 3)}
                width="4"
                height={5 + (itemX % 3) * 3}
                rx="0.6"
              />
              <rect
                className={`office-studio-wall-run-book office-book-${["d", "b", "a"][itemX % 3]}`}
                x={itemX + 6}
                y={55 - (4 + (itemX % 2) * 4)}
                width="3.4"
                height={4 + (itemX % 2) * 4}
                rx="0.6"
              />
              {itemX % 4 === 2 ? (
                <g>
                  <rect
                    className="office-studio-wall-run-box"
                    x={itemX + 10}
                    y="50"
                    width="8"
                    height="5"
                    rx="1"
                  />
                  <path
                    className="office-studio-wall-run-box-line"
                    d={`M${itemX + 12} 52 H${itemX + 16}`}
                  />
                </g>
              ) : (
                <rect
                  className="office-studio-wall-run-object"
                  x={itemX + 11}
                  y="51"
                  width="5"
                  height="4"
                  rx="1"
                />
              )}
            </g>
          ))}
        </g>
        <g
          className="office-studio-sign"
          transform={`translate(${width / 2 - 62} 14)`}
        >
          <rect x="0" y="0" width="124" height="30" rx="4" />
          <path
            className="office-studio-sign-mark"
            d="M12 8 h18 v4 h-12 v3 h10 v4 h-10 v6 h-6 z"
          />
          <path
            className="office-studio-sign-line"
            d="M40 11 h38 M40 17 h26 M40 23 h32"
          />
          <circle className="office-studio-sign-lamp" cx="98" cy="15" r="3" />
        </g>
        {sconceXs.map((sconceX) => (
          <g
            key={`sconce-${sconceX}`}
            className="office-wall-sconce"
            transform={`translate(${sconceX} 28)`}
          >
            <rect
              className="office-wall-sconce-plate"
              x="-4"
              y="-3"
              width="8"
              height="14"
              rx="2"
            />
            <path className="office-room-sconce" d="M-9 10 Q0 1 9 10 Z" />
            <circle
              className="office-room-sconce-bulb"
              cx="0"
              cy="10.5"
              r="3"
            />
            <ellipse
              className="office-sconce-pool"
              cx="0"
              cy="16"
              rx="20"
              ry="10"
            />
          </g>
        ))}
        <path
          className="office-studio-wall-trim office-studio-wall-trim-base"
          d={`M8 ${OFFICE_WALL_HEIGHT} H${width - 8}`}
        />
        <rect
          className="office-studio-skirting"
          x="8"
          y={OFFICE_WALL_HEIGHT}
          width={width - 16}
          height="6"
        />
        <Plant x={18} y={OFFICE_WALL_HEIGHT + 12} scale={0.9} variant="tall" />
        <Plant
          x={width - 18}
          y={OFFICE_WALL_HEIGHT + 12}
          scale={0.84}
          variant="tall"
        />
      </g>

      {columns.map((column, index) => (
        <Plant
          key={`aisle-plant-${column.x}`}
          x={Math.max(22, column.x - 16 + (index === 0 ? 0 : 0))}
          y={OFFICE_WALL_HEIGHT + 14}
          scale={0.62}
          variant="small"
        />
      ))}
      <Plant
        x={width - 22}
        y={OFFICE_WALL_HEIGHT + 14}
        scale={0.62}
        variant="small"
      />
      {showEmblem && <FloorEmblem x={emblemX} y={emblemY} />}

      <rect
        className="office-studio-vignette"
        x="0"
        y="0"
        width={width}
        height={height}
        fill="url(#office-shell-vignette)"
      />
    </svg>
  );
}

export { OFFICE_TOP_PADDING };
