import { OFFICE_ZONE_SIDE_LANE, type OfficeZone } from "./officeSceneLayout";

export function Plant({
  x,
  y,
  scale = 1,
  variant = "standard",
}: {
  x: number;
  y: number;
  scale?: number;
  variant?: "standard" | "tall" | "small";
}) {
  const tall = variant === "tall" ? 1.22 : variant === "small" ? 0.8 : 1;
  return (
    <g
      className={`office-plant office-plant--${variant}`}
      transform={`translate(${x} ${y}) scale(${scale})`}
    >
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="17"
        rx="16"
        ry="3.6"
      />
      <path className="office-plant-pot" d="M-9 3 H9 L6.5 17 H-6.5 Z" />
      <path className="office-plant-pot-shade" d="M3 4 L2 17 H6.5 L9 4 Z" />
      <ellipse
        className="office-plant-pot-rim"
        cx="0"
        cy="3"
        rx="9.5"
        ry="2.6"
      />
      <path
        className="office-plant-stem"
        d={`M0 3 V${-12 * tall} M0 -2 Q-10 -8 -13 ${-20 * tall} M0 -1 Q10 -7 13 ${-19 * tall} M0 -5 Q-4 ${-14 * tall} -3 ${-23 * tall} M0 -4 Q5 ${-13 * tall} 4 ${-22 * tall}`}
      />
      <ellipse
        className="office-plant-leaf"
        cx="-16"
        cy={-21 * tall}
        rx="7.4"
        ry="3.8"
        transform={`rotate(28 -16 ${-21 * tall})`}
      />
      <ellipse
        className="office-plant-leaf"
        cx="-7"
        cy={-28 * tall}
        rx="7.6"
        ry="3.6"
        transform={`rotate(-18 -7 ${-28 * tall})`}
      />
      <ellipse
        className="office-plant-leaf"
        cx="6"
        cy={-29 * tall}
        rx="7.6"
        ry="3.6"
        transform={`rotate(15 6 ${-29 * tall})`}
      />
      <ellipse
        className="office-plant-leaf"
        cx="15"
        cy={-19 * tall}
        rx="6.8"
        ry="3.6"
        transform={`rotate(-26 15 ${-19 * tall})`}
      />
      <ellipse
        className="office-plant-leaf"
        cx="1"
        cy={-14 * tall}
        rx="6.2"
        ry="3.2"
        transform={`rotate(8 1 ${-14 * tall})`}
      />
      <ellipse
        className="office-plant-leaf office-plant-leaf-light"
        cx="-4"
        cy={-23 * tall}
        rx="5"
        ry="2.5"
        transform={`rotate(-10 -4 ${-23 * tall})`}
      />
    </g>
  );
}

function DeskLamp({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-desk-lamp" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow office-desk-lamp-glow"
        cx="0"
        cy="-4"
        rx="26"
        ry="18"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-desk-lamp-base"
        cx="0"
        cy="0"
        rx="7"
        ry="2.2"
      />
      <path className="office-desk-lamp-arm" d="M0 0 V-14 Q0 -19 6 -19" />
      <path className="office-desk-lamp-shade" d="M4 -23 H18 L15 -14 H6 Z" />
      <ellipse
        className="office-desk-lamp-light"
        cx="11"
        cy="-14"
        rx="4.4"
        ry="1.6"
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
        cy="0"
        rx="58"
        ry="38"
        fill={`url(#${glowId})`}
      />
      <ellipse className="office-plant-shadow" cx="0" cy="12" rx="12" ry="3" />
      <ellipse className="office-lamp-base" cx="0" cy="10" rx="9" ry="2.8" />
      <path className="office-lamp-pole" d="M-1 -26 H1 V10 H-1 Z" />
      <path className="office-lamp-arm" d="M0 -24 Q12 -26 14 -17" />
      <path className="office-lamp-shade" d="M4 -28 H23 L20 -17 H7 Z" />
      <ellipse
        className="office-lamp-light"
        cx="13"
        cy="-17"
        rx="6.4"
        ry="2.2"
      />
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
      <path
        className="office-bookshelf-shadow"
        d="M-24 2 L0 10 L24 2 L0 -6 Z"
      />
      <polygon
        className="office-bookshelf-side"
        points="-23,-60 23,-60 29,-54 29,6 23,10 -23,10"
      />
      <rect
        className="office-bookshelf-frame"
        x="-23"
        y="-60"
        width="46"
        height="70"
        rx="2"
      />
      <rect
        className="office-bookshelf-back"
        x="-20"
        y="-57"
        width="40"
        height="62"
        rx="1"
      />
      <path className="office-bookshelf-shelf" d="M-20 -39 H20 M-20 -20 H20" />
      <rect
        className="office-book office-book-a"
        x="-18"
        y="-54"
        width="5"
        height="14"
        rx="0.6"
      />
      <rect
        className="office-book office-book-b"
        x="-11"
        y="-52"
        width="4"
        height="12"
        rx="0.6"
      />
      <rect
        className="office-book office-book-c"
        x="-5"
        y="-55"
        width="6"
        height="15"
        rx="0.6"
      />
      <rect
        className="office-book office-book-d"
        x="4"
        y="-53"
        width="5"
        height="13"
        rx="0.6"
      />
      <rect
        className="office-book office-book-a"
        x="11"
        y="-55"
        width="5"
        height="15"
        rx="0.6"
      />
      <rect
        className="office-book office-book-b"
        x="-18"
        y="-36"
        width="7"
        height="13"
        rx="0.6"
      />
      <rect
        className="office-book office-book-c"
        x="-8"
        y="-34"
        width="5"
        height="11"
        rx="0.6"
      />
      <rect
        className="office-book office-book-d"
        x="-1"
        y="-37"
        width="5"
        height="14"
        rx="0.6"
      />
      <rect
        className="office-book office-book-a"
        x="7"
        y="-35"
        width="6"
        height="12"
        rx="0.6"
      />
      <rect
        className="office-bookshelf-box"
        x="-17"
        y="-16"
        width="15"
        height="11"
        rx="1"
      />
      <circle className="office-bookshelf-plant" cx="10" cy="-13" r="4.4" />
      <path
        className="office-bookshelf-plant-leaf"
        d="M10 -13 q-6 -7 -1 -12 M10 -13 q7 -5 5 -11"
      />
      <path className="office-bookshelf-highlight" d="M-23 -58 V8" />
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
        cy="-26"
        rx="34"
        ry="24"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="14"
        rx="27"
        ry="3.4"
      />
      <path
        className="office-coffee-side"
        d="M-24 -14 H23 L29 -8 V14 L23 18 H-24 Z"
      />
      <rect
        className="office-coffee-body"
        x="-25"
        y="-17"
        width="50"
        height="29"
        rx="2"
      />
      <rect
        className="office-coffee-top"
        x="-27"
        y="-21"
        width="54"
        height="6"
        rx="2"
      />
      <rect
        className="office-coffee-machine"
        x="-16"
        y="-38"
        width="19"
        height="19"
        rx="2"
      />
      <rect
        className="office-coffee-screen"
        x="-13"
        y="-35"
        width="9"
        height="5"
        rx="0.8"
      />
      <circle className="office-coffee-button" cx="-5" cy="-30" r="1.5" />
      <circle
        className="office-coffee-button office-coffee-button-b"
        cx="-5"
        cy="-26"
        r="1.2"
      />
      <path className="office-coffee-spout" d="M-8 -18 v4" />
      <rect
        className="office-coffee-cup"
        x="-11"
        y="-14"
        width="5.4"
        height="5.4"
        rx="1.2"
      />
      <rect
        className="office-coffee-cup"
        x="4"
        y="-16"
        width="4.6"
        height="4.6"
        rx="1"
      />
      <ellipse
        className="office-coffee-jar"
        cx="16"
        cy="-27"
        rx="4.4"
        ry="3.2"
      />
      <path className="office-coffee-jar-lid" d="M12 -30 H20" />
      <path className="office-coffee-warm" d="M-14 -22 h22" />
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
        cy="-24"
        rx="30"
        ry="26"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="14"
        rx="23"
        ry="3.2"
      />
      <rect
        className="office-server-side"
        x="20"
        y="-60"
        width="7"
        height="72"
        rx="1"
      />
      <rect
        className="office-server-body"
        x="-22"
        y="-60"
        width="44"
        height="74"
        rx="2.5"
      />
      <rect
        className="office-server-face"
        x="-18"
        y="-56"
        width="36"
        height="66"
        rx="1.5"
      />
      {Array.from({ length: 8 }, (_, index) => -53 + index * 8).map(
        (unitY, unit) => (
          <g key={`server-unit-${unitY}`}>
            <rect
              className="office-server-unit"
              x="-14"
              y={unitY}
              width="28"
              height="6"
              rx="1"
            />
            <circle
              className="office-server-led"
              cx="-10"
              cy={unitY + 3}
              r="1"
              data-tone={unit % 3}
            />
            <path className="office-server-vent" d={`M-4 ${unitY + 3} H10`} />
          </g>
        ),
      )}
      <path className="office-server-feet" d="M-17 14 V17 M17 14 V17" />
      <rect
        className="office-server-top"
        x="-22"
        y="-63"
        width="44"
        height="4"
        rx="1"
      />
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
        rx="30"
        ry="22"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="14"
        rx="24"
        ry="3.2"
      />
      <rect
        className="office-test-cart-side"
        x="18"
        y="-14"
        width="6"
        height="28"
        rx="1.5"
      />
      <rect
        className="office-test-cart-body"
        x="-22"
        y="-15"
        width="44"
        height="28"
        rx="2"
      />
      <path className="office-test-cart-drawer" d="M-19 -6 H19 M-19 2 H19" />
      <circle className="office-test-cart-wheel" cx="-16" cy="14" r="2.6" />
      <circle className="office-test-cart-wheel" cx="16" cy="14" r="2.6" />
      <rect
        className="office-test-cart-monitor"
        x="-17"
        y="-47"
        width="34"
        height="22"
        rx="2"
      />
      <rect
        className="office-test-cart-screen"
        x="-14"
        y="-44"
        width="28"
        height="16"
        rx="1"
      />
      <path
        className="office-test-cart-trace"
        d="M-11 -36 l4 3 l3 -5 l4 7 l4 -4 l4 2"
      />
      <path className="office-test-cart-stand" d="M-9 -26 V-17 M9 -26 V-17" />
      <rect
        className="office-test-cart-toolbox"
        x="13"
        y="-13"
        width="8"
        height="7"
        rx="1"
      />
      <rect
        className="office-test-cart-top"
        x="-22"
        y="-17"
        width="44"
        height="3"
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
        rx="40"
        ry="26"
        fill={`url(#${glowId})`}
      />
      <ellipse className="office-plant-shadow" cx="0" cy="17" rx="34" ry="4" />
      <path
        className="office-lounge-side"
        d="M-28 -22 L-28 8 L-22 12 L-22 -18 Z"
      />
      <rect
        className="office-lounge-back"
        x="-28"
        y="-27"
        width="56"
        height="21"
        rx="6"
      />
      <rect
        className="office-lounge-seat"
        x="-27"
        y="-11"
        width="54"
        height="14"
        rx="5"
      />
      <rect
        className="office-lounge-cushion"
        x="-22"
        y="-24"
        width="20"
        height="15"
        rx="4"
      />
      <rect
        className="office-lounge-cushion office-lounge-cushion-light"
        x="2"
        y="-24"
        width="20"
        height="15"
        rx="4"
      />
      <rect
        className="office-lounge-arm"
        x="-32"
        y="-16"
        width="7"
        height="21"
        rx="2.5"
      />
      <rect
        className="office-lounge-arm"
        x="25"
        y="-16"
        width="7"
        height="21"
        rx="2.5"
      />
      <path className="office-lounge-leg" d="M-22 2 V10 M22 2 V10" />
      <ellipse className="office-lounge-table" cx="0" cy="17" rx="20" ry="5" />
      <rect
        className="office-lounge-table-top"
        x="-20"
        y="13"
        width="40"
        height="4.4"
        rx="2"
      />
      <rect
        className="office-lounge-book"
        x="-9"
        y="9"
        width="10"
        height="3.4"
        rx="0.8"
      />
      <rect
        className="office-lounge-cup"
        x="8"
        y="8"
        width="4.4"
        height="4.4"
        rx="1.2"
      />
      <path className="office-lounge-cushion-seam" d="M-22 -18 v9 M22 -18 v9" />
    </g>
  );
}

function Cabinet({ x, y, glowId }: { x: number; y: number; glowId: string }) {
  return (
    <g className="office-cabinet" transform={`translate(${x} ${y})`}>
      <ellipse
        className="office-lamp-glow"
        cx="0"
        cy="-16"
        rx="24"
        ry="18"
        fill={`url(#${glowId})`}
      />
      <ellipse
        className="office-plant-shadow"
        cx="0"
        cy="11"
        rx="18"
        ry="2.8"
      />
      <rect
        className="office-cabinet-side"
        x="14"
        y="-34"
        width="5"
        height="45"
        rx="1"
      />
      <rect
        className="office-cabinet-body"
        x="-16"
        y="-34"
        width="32"
        height="45"
        rx="2"
      />
      <path
        className="office-cabinet-drawer"
        d="M-13 -27 H13 M-13 -13 H13 M-13 1 H13"
      />
      <rect
        className="office-cabinet-handle"
        x="-3"
        y="-22"
        width="6"
        height="2"
        rx="1"
      />
      <rect
        className="office-cabinet-handle"
        x="-3"
        y="-8"
        width="6"
        height="2"
        rx="1"
      />
      <rect
        className="office-cabinet-top"
        x="-16"
        y="-36"
        width="32"
        height="3"
        rx="1"
      />
    </g>
  );
}

function roleFurniture(zone: OfficeZone, glowId: string) {
  const laneX = zone.width - OFFICE_ZONE_SIDE_LANE / 2 - 4;
  const floorBottom = zone.height - 12;
  switch (zone.role) {
    case "LEAD":
      return (
        <>
          <Bookshelf x={laneX + 2} y={floorBottom - 26} scale={0.94} />
          <DeskLamp x={laneX - 6} y={floorBottom - 58} glowId={glowId} />
          <Plant
            x={laneX - 8}
            y={floorBottom + 2}
            scale={0.6}
            variant="small"
          />
        </>
      );
    case "IMPLEMENTER":
      return (
        <>
          <CoffeeStation x={laneX} y={floorBottom - 46} glowId={glowId} />
          <Plant
            x={laneX - 10}
            y={floorBottom + 2}
            scale={0.66}
            variant="small"
          />
          <g
            className="office-bin"
            transform={`translate(${laneX + 18} ${floorBottom - 2})`}
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
          <TestCart x={laneX} y={floorBottom - 40} glowId={glowId} />
          <Cabinet x={laneX - 22} y={floorBottom + 2} glowId={glowId} />
          <Plant
            x={laneX + 4}
            y={floorBottom + 2}
            scale={0.58}
            variant="small"
          />
        </>
      );
    case "REVIEWER":
      return (
        <>
          <Lounge x={laneX - 4} y={floorBottom - 18} glowId={glowId} />
          <Plant
            x={laneX + 8}
            y={floorBottom + 2}
            scale={0.56}
            variant="small"
          />
        </>
      );
    case "WORKER":
      return (
        <>
          <ServerRack x={laneX} y={floorBottom - 10} glowId={glowId} />
          <Plant
            x={laneX - 12}
            y={floorBottom + 2}
            scale={0.62}
            variant="small"
          />
          <g
            className="office-ups"
            transform={`translate(${laneX + 26} ${floorBottom - 2})`}
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
      {roleFurniture(zone, glowId)}
      <Plant x={20} y={zone.height - 4} scale={0.58} variant="small" />
      <FloorLamp x={zone.width - 18} y={zone.height - 20} glowId={glowId} />
    </>
  );
}
