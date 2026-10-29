import {
  OFFICE_WALL_HEIGHT,
  OFFICE_ZONE_SIDE_LANE,
  type OfficeZone,
} from "./officeSceneLayout";

function Plant({ x, y, scale = 1 }: { x: number; y: number; scale?: number }) {
  return (
    <g
      className="office-plant"
      transform={`translate(${x} ${y}) scale(${scale})`}
    >
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="18"
        rx="14"
        ry="3.2"
      />
      <path className="office-plant-pot" d="M-8 4 H8 L6 17 H-6 Z" />
      <path className="office-plant-pot-rim" d="M-9 3 H9 V6 H-9 Z" />
      <path
        className="office-plant-stem"
        d="M0 4 V-10 M0 -2 Q-9 -8 -11 -18 M0 -1 Q9 -7 11 -17 M0 -5 Q-4 -13 -3 -20 M0 -4 Q5 -12 4 -19"
      />
      <ellipse
        className="office-plant-leaf"
        cx="-14"
        cy="-19"
        rx="6.4"
        ry="3.4"
        transform="rotate(26 -14 -19)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="-6"
        cy="-26"
        rx="6.8"
        ry="3.2"
        transform="rotate(-20 -6 -26)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="5"
        cy="-27"
        rx="6.8"
        ry="3.2"
        transform="rotate(16 5 -27)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="13"
        cy="-17"
        rx="6"
        ry="3.2"
        transform="rotate(-28 13 -17)"
      />
      <ellipse
        className="office-plant-leaf"
        cx="1"
        cy="-12"
        rx="5.6"
        ry="3"
        transform="rotate(8 1 -12)"
      />
      <ellipse
        className="office-plant-leaf office-plant-leaf-light"
        cx="-4"
        cy="-21"
        rx="4.4"
        ry="2.2"
        transform="rotate(-10 -4 -21)"
      />
    </g>
  );
}

function FloorLamp({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-floor-lamp" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="2"
        rx="52"
        ry="34"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="12"
        rx="11"
        ry="2.8"
      />
      <ellipse className="office-lamp-base" cx="0" cy="10" rx="8" ry="2.4" />
      <rect
        className="office-lamp-pole"
        x="-1.2"
        y="-22"
        width="2.4"
        height="32"
        rx="1.2"
      />
      <path className="office-lamp-arm" d="M0 -20 Q10 -22 12 -15" />
      <path className="office-lamp-shade" d="M3 -24 H21 L18 -14 H6 Z" />
      <ellipse className="office-lamp-light" cx="12" cy="-14" rx="6" ry="2" />
    </g>
  );
}

function Bookshelf({
  x,
  y,
  scale = 1,
}: {
  x: number;
  y: number;
  scale?: number;
}) {
  return (
    <g
      className="office-bookshelf"
      transform={`translate(${x} ${y}) scale(${scale})`}
    >
      <rect
        className="office-bookshelf-frame"
        x="-23"
        y="-56"
        width="46"
        height="56"
        rx="2"
      />
      <rect
        className="office-bookshelf-back"
        x="-20"
        y="-53"
        width="40"
        height="50"
        rx="1"
      />
      <path className="office-bookshelf-shelf" d="M-20 -36 H20 M-20 -19 H20" />
      <rect
        className="office-book office-book-a"
        x="-18"
        y="-50"
        width="5"
        height="13"
        rx="0.6"
      />
      <rect
        className="office-book office-book-b"
        x="-11"
        y="-48"
        width="4"
        height="11"
        rx="0.6"
      />
      <rect
        className="office-book office-book-c"
        x="-5"
        y="-51"
        width="6"
        height="14"
        rx="0.6"
      />
      <rect
        className="office-book office-book-d"
        x="4"
        y="-49"
        width="5"
        height="12"
        rx="0.6"
      />
      <rect
        className="office-book office-book-a"
        x="11"
        y="-51"
        width="5"
        height="14"
        rx="0.6"
      />
      <rect
        className="office-book office-book-b"
        x="-18"
        y="-33"
        width="7"
        height="12"
        rx="0.6"
      />
      <rect
        className="office-book office-book-c"
        x="-8"
        y="-31"
        width="5"
        height="10"
        rx="0.6"
      />
      <rect
        className="office-book office-book-d"
        x="-1"
        y="-34"
        width="5"
        height="13"
        rx="0.6"
      />
      <rect
        className="office-book office-book-a"
        x="7"
        y="-32"
        width="6"
        height="11"
        rx="0.6"
      />
      <rect
        className="office-bookshelf-box"
        x="-17"
        y="-15"
        width="15"
        height="10"
        rx="1"
      />
      <circle className="office-bookshelf-plant" cx="10" cy="-12" r="4" />
      <path
        className="office-bookshelf-plant-leaf"
        d="M10 -12 q-5 -6 -1 -10 M10 -12 q6 -4 4 -9"
      />
    </g>
  );
}

function CoffeeStation({
  x,
  y,
  glowId,
}: {
  x: number;
  y: number;
  glowId: string;
}) {
  return (
    <g className="office-coffee" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="-24"
        rx="30"
        ry="22"
        fill={`url(#${glowId})`}
      />
      <ellipse className="office-plant-shadow" cx="0" cy="13" rx="24" ry="3" />
      <rect
        className="office-coffee-body"
        x="-23"
        y="-13"
        width="46"
        height="25"
        rx="2"
      />
      <rect
        className="office-coffee-top"
        x="-25"
        y="-16"
        width="50"
        height="5"
        rx="1.5"
      />
      <rect
        className="office-coffee-machine"
        x="-15"
        y="-31"
        width="17"
        height="17"
        rx="2"
      />
      <rect
        className="office-coffee-screen"
        x="-12"
        y="-28"
        width="8"
        height="4"
        rx="0.8"
      />
      <circle className="office-coffee-button" cx="-5" cy="-24" r="1.4" />
      <path className="office-coffee-spout" d="M-7 -14 v4" />
      <rect
        className="office-coffee-cup"
        x="-10"
        y="-10"
        width="5"
        height="5"
        rx="1"
      />
      <rect
        className="office-coffee-cup"
        x="4"
        y="-12"
        width="4.4"
        height="4.4"
        rx="1"
      />
      <ellipse className="office-coffee-jar" cx="14" cy="-21" rx="4" ry="3" />
    </g>
  );
}

function ServerRack({
  x,
  y,
  glowId,
}: {
  x: number;
  y: number;
  glowId: string;
}) {
  return (
    <g className="office-server-rack" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="-22"
        rx="28"
        ry="24"
        fill={`url(#${glowId})`}
      />
      <ellipse className="office-plant-shadow" cx="0" cy="13" rx="21" ry="3" />
      <rect
        className="office-server-body"
        x="-20"
        y="-58"
        width="40"
        height="70"
        rx="2.5"
      />
      <rect
        className="office-server-face"
        x="-16"
        y="-54"
        width="32"
        height="62"
        rx="1.5"
      />
      {Array.from({ length: 7 }, (_, index) => -51 + index * 8).map(
        (unitY, unit) => (
          <g key={`server-unit-${unitY}`}>
            <rect
              className="office-server-unit"
              x="-13"
              y={unitY}
              width="26"
              height="6"
              rx="1"
            />
            <circle
              className="office-server-led"
              cx="-9"
              cy={unitY + 3}
              r="1"
              data-tone={unit % 3}
            />
            <path className="office-server-vent" d={`M-4 ${unitY + 3} H9`} />
          </g>
        ),
      )}
      <path className="office-server-feet" d="M-16 12 V15 M16 12 V15" />
    </g>
  );
}

function TestCart({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-test-cart" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="-28"
        rx="27"
        ry="20"
        fill={`url(#${glowId})`}
      />
      <ellipse className="office-plant-shadow" cx="0" cy="13" rx="22" ry="3" />
      <rect
        className="office-test-cart-body"
        x="-21"
        y="-14"
        width="42"
        height="26"
        rx="2"
      />
      <path className="office-test-cart-drawer" d="M-18 -6 H18 M-18 2 H18" />
      <circle className="office-test-cart-wheel" cx="-15" cy="13" r="2.4" />
      <circle className="office-test-cart-wheel" cx="15" cy="13" r="2.4" />
      <rect
        className="office-test-cart-monitor"
        x="-16"
        y="-45"
        width="32"
        height="20"
        rx="2"
      />
      <rect
        className="office-test-cart-screen"
        x="-13"
        y="-42"
        width="26"
        height="14"
        rx="1"
      />
      <path
        className="office-test-cart-trace"
        d="M-10 -35 l4 3 l3 -5 l4 7 l4 -4 l4 2"
      />
      <path className="office-test-cart-stand" d="M-8 -25 V-16 M8 -25 V-16" />
      <rect
        className="office-test-cart-toolbox"
        x="12"
        y="-12"
        width="8"
        height="7"
        rx="1"
      />
    </g>
  );
}

function Lounge({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-lounge" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="-18"
        rx="34"
        ry="24"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="15"
        rx="28"
        ry="3.4"
      />
      <rect
        className="office-lounge-back"
        x="-27"
        y="-25"
        width="54"
        height="19"
        rx="5"
      />
      <rect
        className="office-lounge-seat"
        x="-25"
        y="-11"
        width="50"
        height="12"
        rx="4"
      />
      <rect
        className="office-lounge-cushion"
        x="-20"
        y="-22"
        width="18"
        height="13"
        rx="3"
      />
      <rect
        className="office-lounge-cushion office-lounge-cushion-light"
        x="2"
        y="-22"
        width="18"
        height="13"
        rx="3"
      />
      <rect
        className="office-lounge-arm"
        x="-30"
        y="-14"
        width="6"
        height="18"
        rx="2"
      />
      <rect
        className="office-lounge-arm"
        x="24"
        y="-14"
        width="6"
        height="18"
        rx="2"
      />
      <path className="office-lounge-leg" d="M-20 1 V8 M20 1 V8" />
      <ellipse
        className="office-lounge-table"
        cx="0"
        cy="14"
        rx="18"
        ry="4.4"
      />
      <rect
        className="office-lounge-table-top"
        x="-18"
        y="10"
        width="36"
        height="4"
        rx="2"
      />
      <rect
        className="office-lounge-book"
        x="-8"
        y="7"
        width="9"
        height="3"
        rx="0.6"
      />
      <rect
        className="office-lounge-cup"
        x="7"
        y="6"
        width="4"
        height="4"
        rx="1"
      />
    </g>
  );
}

function PendantLights({ width, glowId }: { width: number; glowId: string }) {
  const count = width > 600 ? 3 : width > 360 ? 2 : 1;
  const start = width * 0.28;
  const end = width * 0.72;
  return (
    <g className="office-pendants">
      {Array.from({ length: count }, (_, index) => {
        const x =
          count === 1
            ? width / 2
            : start + ((end - start) * index) / (count - 1);
        return (
          <g
            key={`pendant-${x}`}
            transform={`translate(${x} ${OFFICE_WALL_HEIGHT + 2})`}
          >
            <path className="office-pendant-cord" d="M0 0 V12" />
            <path className="office-pendant-shade" d="M-9 12 H9 L6 20 H-6 Z" />
            <ellipse
              className="office-pendant-light"
              cx="0"
              cy="20"
              rx="5"
              ry="1.8"
            />
            <ellipse
              className="office-pendant-glow"
              cx="0"
              cy="28"
              rx="34"
              ry="24"
              fill={`url(#${glowId})`}
            />
          </g>
        );
      })}
    </g>
  );
}

function roleFurniture(zone: OfficeZone, glowId: string) {
  const laneX = zone.width - OFFICE_ZONE_SIDE_LANE / 2 - 6;
  const floorBottom = zone.height - 20;
  switch (zone.role) {
    case "LEAD":
      return (
        <>
          <Bookshelf x={laneX + 4} y={floorBottom - 28} scale={0.92} />
          <Plant x={laneX - 8} y={floorBottom + 2} scale={0.6} />
        </>
      );
    case "IMPLEMENTER":
      return (
        <>
          <CoffeeStation x={laneX} y={floorBottom - 52} glowId={glowId} />
          <Plant x={laneX - 8} y={floorBottom + 2} scale={0.7} />
          <g
            className="office-bin"
            transform={`translate(${laneX + 16} ${floorBottom - 4})`}
          >
            <path className="office-bin-body" d="M-7 -13 H7 L5 0 H-5 Z" />
            <path className="office-bin-lid" d="M-8 -15 H8 V-12 H-8 Z" />
            <path className="office-bin-line" d="M-2 -10 V-2 M2 -10 V-2" />
          </g>
        </>
      );
    case "VERIFIER":
      return (
        <>
          <TestCart x={laneX} y={floorBottom - 44} glowId={glowId} />
          <Plant x={laneX - 10} y={floorBottom + 2} scale={0.62} />
        </>
      );
    case "REVIEWER":
      return (
        <>
          <Lounge x={laneX} y={floorBottom - 22} glowId={glowId} />
          <Plant x={laneX - 10} y={floorBottom + 2} scale={0.58} />
        </>
      );
    case "WORKER":
      return (
        <>
          <ServerRack x={laneX} y={floorBottom - 14} glowId={glowId} />
          <Plant x={laneX - 12} y={floorBottom + 2} scale={0.66} />
          <g
            className="office-ups"
            transform={`translate(${laneX + 28} ${floorBottom - 2})`}
          >
            <rect
              className="office-ups-body"
              x="-8"
              y="-20"
              width="16"
              height="20"
              rx="2"
            />
            <rect
              className="office-ups-screen"
              x="-5"
              y="-17"
              width="10"
              height="4"
              rx="1"
            />
            <circle className="office-ups-led" cx="-3" cy="-9" r="1.2" />
            <circle
              className="office-ups-led office-ups-led-b"
              cx="1"
              cy="-9"
              r="1.2"
            />
          </g>
        </>
      );
  }
}

export function RoomFloorFurniture({
  zone,
  glowId,
}: {
  zone: OfficeZone;
  glowId: string;
}) {
  return (
    <>
      <PendantLights width={zone.width} glowId={glowId} />
      {roleFurniture(zone, glowId)}
      <Plant x={22} y={zone.height - 18} scale={0.74} />
      <FloorLamp x={zone.width - 18} y={zone.height - 30} glowId={glowId} />
    </>
  );
}
