import type { ParticipantRole } from "../../coreApi";
import type { OfficeAvatarLook } from "./officeAvatarAppearance";

/**
 * Original 3/4-view figure geometry for the Agent Office.
 *
 * The figure is drawn in a 92 × 112 local coordinate space. It has no
 * animation, pose change, tool use or progress: a CURRENT observation uses the
 * seated studio pose, every other observation uses the standing pose with an
 * observation ring. Appearance is derived only from the stable look index.
 *
 * Seated figures are drawn from the studio camera behind the workstation: the
 * operator sees the agent's back, chair and the rear of the monitors. That is
 * a rendering orientation only; it does not imply a session is active beyond
 * the recorded CURRENT observation.
 */

type HairProps = { look: OfficeAvatarLook };

function HairBack({ look }: HairProps) {
  switch (look.hairStyle) {
    case 2:
      return (
        <path
          className="office-avatar-hair office-avatar-hair-back"
          d="M35 26 Q35 12 46 12 Q57 12 57 26 L57 39 Q54 42 52 38 L52 25 Q49 21 46 21 Q43 21 40 25 L40 38 Q38 42 35 39 Z"
        />
      );
    case 3:
      return (
        <>
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M36 27 Q36 13 46 13 Q56 13 56 27 L55 33 L37 33 Z"
          />
          <circle
            className="office-avatar-hair office-avatar-hair-back"
            cx="56"
            cy="16"
            r="5.4"
          />
          <circle className="office-avatar-hair-tie" cx="56" cy="21" r="1.3" />
        </>
      );
    case 4:
      return (
        <g className="office-avatar-hair office-avatar-hair-back">
          <circle cx="37" cy="21" r="4.4" />
          <circle cx="55" cy="21" r="4.4" />
          <circle cx="40" cy="14" r="4.6" />
          <circle cx="52" cy="14" r="4.6" />
          <circle cx="46" cy="12" r="4.8" />
          <circle cx="35" cy="27" r="3.4" />
          <circle cx="57" cy="27" r="3.4" />
        </g>
      );
    case 5:
      return (
        <>
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M36 27 Q36 13 46 13 Q56 13 56 27 L54 32 L38 32 Z"
          />
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M54 19 Q63 22 62 33 Q61 39 57 36 Q61 29 55 23 Z"
          />
        </>
      );
    default:
      return (
        <path
          className="office-avatar-hair office-avatar-hair-back"
          d="M37 27 Q37 14 46 14 Q55 14 55 27 L53 33 L39 33 Z"
        />
      );
  }
}

function HairFront({ look }: HairProps) {
  switch (look.hairStyle) {
    case 1:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M36.4 27 Q36 15 46 15 Q55 15 55.6 26 Q52 19 47 19 Q42 20 36.4 27 Z"
          />
          <path
            className="office-avatar-hair"
            d="M38 20 Q43 15 50 16 Q53 17 54.4 21 Q50 18 45 20 Q41 22 38 25 Z"
          />
        </>
      );
    case 2:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M36 27 Q36 15 46 15 Q56 15 56 27 Q53 21 49 20 Q44 18 40 22 Q37.6 24 36 27 Z"
          />
          <path
            className="office-avatar-hair"
            d="M36 26 Q34 31 36.4 38 Q38 40 39.4 37 Q38 32 39 28 Z"
          />
          <path
            className="office-avatar-hair"
            d="M56 26 Q58 31 55.6 38 Q54 40 52.6 37 Q54 32 53 28 Z"
          />
        </>
      );
    case 3:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M36.6 27 Q36.4 14 46 14 Q55.6 14 55.4 27 Q52 20 46 20 Q39.6 20 36.6 27 Z"
          />
          <path
            className="office-avatar-hair"
            d="M37 20 Q42 15 46 17 Q48 18 49 20 Q44 18 40 21 Z"
          />
        </>
      );
    case 4:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M37 27 Q37 15 46 15 Q55 15 55 27 Q52 20 46 20 Q40 20 37 27 Z"
          />
          <circle className="office-avatar-hair" cx="40" cy="17" r="3.6" />
          <circle className="office-avatar-hair" cx="46" cy="15" r="3.8" />
          <circle className="office-avatar-hair" cx="52" cy="17" r="3.6" />
          <circle className="office-avatar-hair" cx="37.4" cy="22" r="2.8" />
          <circle className="office-avatar-hair" cx="54.6" cy="22" r="2.8" />
        </>
      );
    case 5:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M36.4 27 Q36 14 46 14 Q55 14 55.6 25 Q52 19 46 19 Q41 20 36.4 27 Z"
          />
          <path
            className="office-avatar-hair"
            d="M37 21 Q41 16 46 17 Q45 20 41 22 Q39 23 37 25 Z"
          />
        </>
      );
    default:
      return (
        <path
          className="office-avatar-hair"
          d="M36.6 27 Q36.6 15 46 15 Q55.4 15 55.4 27 Q52 20 46 20 Q40 20 36.6 27 Z"
        />
      );
  }
}

function Accessory({ look }: HairProps) {
  const glasses = look.accessory === 1 || look.accessory === 3;
  const headphones = look.accessory === 2 || look.accessory === 3;
  return (
    <>
      {glasses && (
        <g className="office-avatar-glasses">
          <rect x="39.4" y="26.3" width="5.4" height="4" rx="1.4" />
          <rect x="47.2" y="26.1" width="5" height="3.8" rx="1.4" />
          <path d="M44.8 27.3 H47.2" />
          <path d="M39.4 27.4 H37.8" />
        </g>
      )}
      {headphones && (
        <g className="office-avatar-headphones">
          <path d="M36.4 28 Q35.6 14 46 13 Q56.4 14 55.6 28" />
          <rect x="34.4" y="26" width="3.6" height="6.6" rx="1.7" />
          <rect x="54" y="26" width="3.6" height="6.6" rx="1.7" />
          <path
            className="office-avatar-headphones-cord"
            d="M36.2 32 Q35 39 38 42"
          />
        </g>
      )}
    </>
  );
}

function FacialHair({ look }: HairProps) {
  if (look.facialHair === 1) {
    return (
      <path
        className="office-avatar-facial-hair"
        d="M37.6 30 Q37.8 39.4 46 41.4 Q54.2 39.4 54.4 30 Q53 35.4 49.4 36.4 L46 38.8 L42.6 36.4 Q39 35.4 37.6 30 Z"
      />
    );
  }
  if (look.facialHair === 2) {
    return (
      <>
        <path
          className="office-avatar-facial-hair"
          d="M42.6 36.6 Q44.2 35.8 46 38.6 Q47.8 35.8 49.4 36.6 Q49 41 46 41.6 Q43 41 42.6 36.6 Z"
        />
        <path
          className="office-avatar-facial-hair"
          d="M42.6 32.6 Q46 31.4 49.4 32.6 Q48 34 46 34 Q44 34 42.6 32.6 Z"
        />
      </>
    );
  }
  if (look.facialHair === 3) {
    return (
      <path
        className="office-avatar-facial-hair"
        d="M42.2 32.6 Q46 31 49.8 32.6 Q48.4 34.2 46 34.1 Q43.6 34.2 42.2 32.6 Z"
      />
    );
  }
  if (look.facialHair === 4) {
    return (
      <path
        className="office-avatar-stubble"
        d="M37.8 30 Q38.4 39 46 41 Q53.6 39 54.2 30 Q52.8 35.6 49 36.6 L46 38.8 L43 36.6 Q39.2 35.6 37.8 30 Z"
      />
    );
  }
  return null;
}

function Face({ look }: HairProps) {
  return (
    <g className="office-avatar-face">
      <ellipse
        className="office-avatar-ear"
        cx="37.4"
        cy="29.2"
        rx="1.8"
        ry="2.5"
      />
      <ellipse
        className="office-avatar-ear office-avatar-ear-far"
        cx="54.2"
        cy="28.8"
        rx="1.3"
        ry="2.1"
      />
      <FacialHair look={look} />
      <path
        className="office-avatar-brow"
        d="M40.6 25.2 Q42.4 24.3 44.2 25.1"
      />
      <path className="office-avatar-brow" d="M47.8 25 Q49.4 24.2 51 25" />
      <ellipse
        className="office-avatar-eye"
        cx="42.4"
        cy="27.6"
        rx="0.95"
        ry="1.1"
      />
      <ellipse
        className="office-avatar-eye"
        cx="49.4"
        cy="27.4"
        rx="0.8"
        ry="1"
      />
      <path
        className="office-avatar-nose"
        d="M46.2 27.8 Q45.2 30 45.6 31.4 Q46.4 31.9 47.2 31.5"
      />
      <path
        className="office-avatar-mouth"
        d="M43.4 33.6 Q45.4 34.8 47.8 33.7"
      />
      <path
        className="office-avatar-face-shade"
        d="M49 20 Q54 23 53.6 30 Q53.2 35.6 49 38 Q52 34 51.6 28 Q51.2 23 49 20 Z"
      />
      <path
        className="office-avatar-face-light"
        d="M39 29 Q39.4 34 42.4 36.6 Q40.6 33 40.8 29 Z"
      />
    </g>
  );
}

function Head({ look }: HairProps) {
  return (
    <>
      <HairBack look={look} />
      <ellipse
        className="office-avatar-head office-avatar-head-3q"
        cx="46"
        cy="27"
        rx="8.2"
        ry="9.6"
      />
      <path
        className="office-avatar-jaw-shade"
        d="M38.4 27 Q39.2 35.4 46 37.8 Q52.8 35.4 53.6 27 Q52.4 34 46 35.6 Q39.6 34 38.4 27 Z"
      />
      <Face look={look} />
      <HairFront look={look} />
      <path
        className="office-avatar-hair-sheen"
        d="M38.6 18.4 Q42 13.6 47.4 14.4 Q43.2 16.2 40.6 20.6 Z"
      />
      <Accessory look={look} />
    </>
  );
}

/** Rear three-quarter head used by the seated studio pose. */
function SeatedHead({ look }: HairProps) {
  const headphones = look.accessory === 2 || look.accessory === 3;
  const glasses = look.accessory === 1 || look.accessory === 3;
  return (
    <>
      <ellipse
        className="office-avatar-head office-avatar-head-back"
        cx="46"
        cy="27"
        rx="8.4"
        ry="9.8"
      />
      <HairBack look={look} />
      <path
        className="office-avatar-hair-sheen"
        d="M38.4 21 Q40.4 14.4 46 13.6 Q42.6 17 41 23.2 Z"
      />
      <path
        className="office-avatar-head-shade"
        d="M49 19 Q55 22 54.4 30 Q53.6 36 49 39 Q52.6 34 52.6 29 Q52.6 23 49 19 Z"
      />
      <path
        className="office-avatar-cheek"
        d="M52 27 Q55 29 54 33 Q53 36 51 35 Q53 32 52.6 29 Z"
      />
      <ellipse
        className="office-avatar-ear"
        cx="37.4"
        cy="28.6"
        rx="2.1"
        ry="2.9"
      />
      <path className="office-avatar-ear-inner" d="M37 27.4 Q38 28.6 37 30.4" />
      {glasses && (
        <path
          className="office-avatar-glasses office-avatar-glasses-temple"
          d="M37.8 26.6 Q35.4 26.8 34.2 27.6"
        />
      )}
      {headphones && (
        <g className="office-avatar-headphones">
          <path d="M35.8 27.6 Q35 14 46 13 Q57 14 56.2 27.6" />
          <rect x="33.8" y="25.4" width="3.8" height="7" rx="1.8" />
          <rect x="54.4" y="25.4" width="3.8" height="7" rx="1.8" />
          <path
            className="office-avatar-headphones-cord"
            d="M35.6 32 Q34.4 39 37.6 42.4"
          />
        </g>
      )}
      <path
        className="office-avatar-neck-shade"
        d="M42 35 H50 V43 Q46 46 42 43 Z"
      />
      {look.hairStyle === 0 && (
        <path className="office-avatar-hair-part" d="M46 14 V21" />
      )}
    </>
  );
}

function TorsoAndArms({
  look,
  seated,
}: {
  look: OfficeAvatarLook;
  seated: boolean;
}) {
  const width = look.build === 1 ? 1.4 : look.build === 3 ? -2 : 0;
  const shoulders = 28 + width;
  const shouldersRight = 64 - width;
  const torso = seated
    ? `M${shoulders} 50 Q46 41 ${shouldersRight} 50 L${66 - width} 78 Q46 83 ${26 + width} 78 Z`
    : `M${shoulders} 45 Q46 37 ${shouldersRight} 45 L${64 - width} 77 Q46 82 ${28 + width} 77 Z`;
  const leftArm = seated
    ? `M${shoulders + 3} 54 Q${30 + width} 67 ${35 + width} 82`
    : `M${shoulders + 2} 50 Q${22 + width} 63 ${25 + width} 76`;
  const rightArm = seated
    ? `M${shouldersRight - 3} 54 Q${62 - width} 67 ${57 - width} 82`
    : `M${shouldersRight - 2} 50 Q${70 - width} 63 ${67 - width} 76`;
  return (
    <>
      <path className="office-avatar-torso" d={torso} />
      <path
        className="office-avatar-torso-shade"
        d={
          seated
            ? `M54 48 Q60 52 66 78 L58 82 Q58 62 50 52 Z`
            : `M53 46 Q59 51 64 77 L57 81 Q57 61 49 51 Z`
        }
      />
      {look.jacket > 0 && (
        <g className="office-avatar-jacket">
          <path
            d={
              seated
                ? `M${shoulders + 2} 50 Q36 44 42 49 L40 78 L${26 + width} 78 Z`
                : `M${shoulders + 2} 46 Q36 40 42 46 L40 77 L${28 + width} 77 Z`
            }
          />
          <path
            d={
              seated
                ? `M${shouldersRight - 2} 50 Q56 44 50 49 L52 78 L${66 - width} 78 Z`
                : `M${shouldersRight - 2} 46 Q56 40 50 46 L52 77 L${64 - width} 77 Z`
            }
          />
        </g>
      )}
      {seated ? (
        <>
          <path className="office-avatar-back-seam" d="M46 47 V78" />
          <path className="office-avatar-collar" d="M40 47 Q46 52 52 47" />
        </>
      ) : (
        <path
          className="office-avatar-shirt-detail"
          d="M46 41 L43 59 L46 66 L49 59 Z"
        />
      )}
      <path className="office-avatar-arm office-avatar-arm-left" d={leftArm} />
      <path
        className="office-avatar-arm office-avatar-arm-right"
        d={rightArm}
      />
      <path
        className="office-avatar-cuff"
        d={seated ? `M${33 + width} 77 l2.4 3.8` : `M${24 + width} 73 l2.6 4.4`}
      />
      <path
        className="office-avatar-cuff"
        d={
          seated ? `M${59 - width} 77 l-2.4 3.8` : `M${68 - width} 73 l-2.6 4.4`
        }
      />
      {!seated && (
        <>
          <circle
            className="office-avatar-hand"
            cx={28 + width}
            cy={78}
            r="3.2"
          />
          <circle
            className="office-avatar-hand"
            cx={64 - width}
            cy={78}
            r="3.2"
          />
        </>
      )}
      <path
        className="office-avatar-rim"
        d={
          seated
            ? `M36 52 Q38 45 46 42 Q40 48 39 58`
            : `M35 49 Q37 42 46 39 Q40 45 39 56`
        }
      />
    </>
  );
}

function Badge({ seated }: { seated: boolean }) {
  const top = seated ? 52 : 47;
  return (
    <g className="office-avatar-badge-group">
      <path
        className="office-avatar-lanyard"
        d={`M42 ${top} Q46 ${top + 8} 50 ${top}`}
      />
      <rect
        className="office-avatar-badge"
        x="48"
        y={top + 8}
        width="4.2"
        height="5.2"
        rx="0.8"
      />
    </g>
  );
}

function Chair({ look }: HairProps) {
  return (
    <g className="office-avatar-chair-group">
      <path
        className="office-avatar-chair-back-shade"
        d="M28 54 Q28 46 46 46 Q64 46 64 54 L64 78 Q46 84 28 78 Z"
      />
      <rect
        className="office-avatar-chair"
        x="29"
        y="49"
        width="34"
        height="31"
        rx="10"
      />
      <path
        className="office-avatar-chair-seam"
        d="M34 57 H58 M34 64 H58 M35 71 H57"
      />
      <path className="office-avatar-chair-post" d="M45 80 V99" />
      <path className="office-avatar-chair-base" d="M32 107 L45 101 L60 107" />
      <circle className="office-avatar-wheel" cx="31" cy="108" r="2.4" />
      <circle className="office-avatar-wheel" cx="45" cy="109.6" r="2.4" />
      <circle className="office-avatar-wheel" cx="61" cy="108" r="2.4" />
      <path className="office-avatar-chair-arm" d="M27 59 Q24 67 28 74" />
      <path className="office-avatar-chair-arm" d="M65 59 Q68 67 64 74" />
      <circle className="office-avatar-chair-arm-cap" cx="28" cy="74" r="2.2" />
      <circle className="office-avatar-chair-arm-cap" cx="64" cy="74" r="2.2" />
      <circle className="office-avatar-chair-logo" cx="46" cy="61" r="1.7" />
      {look.jacket > 0 && (
        <path className="office-avatar-chair-accent" d="M33 52 H59" />
      )}
    </g>
  );
}

function MonitorBack({
  x,
  y,
  width,
  height,
  warm,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
  warm?: boolean;
}) {
  return (
    <g className="office-avatar-monitor-unit">
      <rect
        className="office-avatar-monitor office-avatar-monitor-back"
        x={x}
        y={y}
        width={width}
        height={height}
        rx="1.8"
      />
      <path
        className="office-avatar-monitor-rim"
        d={`M${x + 1.4} ${y + height - 2} V${y + 2} H${x + width - 1.4}`}
      />
      <path
        className="office-avatar-monitor-vent"
        d={`M${x + width / 2 - 4} ${y + height * 0.62} H${x + width / 2 + 4} M${x + width / 2 - 3} ${y + height * 0.72} H${x + width / 2 + 3}`}
      />
      <circle
        className="office-avatar-monitor-led"
        data-warm={warm ? "true" : "false"}
        cx={x + width / 2}
        cy={y + height - 4}
        r="1.1"
      />
      <rect
        className="office-avatar-monitor-stand"
        x={x + width / 2 - 1.4}
        y={y + height}
        width="2.8"
        height="6"
      />
      <path
        className="office-avatar-monitor-foot"
        d={`M${x + width / 2 - 6} ${y + height + 6} H${x + width / 2 + 6}`}
      />
    </g>
  );
}

function WorkstationDesk({
  role,
  look,
}: {
  role: ParticipantRole;
  look: OfficeAvatarLook;
}) {
  return (
    <g className="office-avatar-workstation">
      <path
        className="office-avatar-desk-shadow"
        d="M14 95 L46 84 L78 95 L46 106 Z"
      />
      <path
        className="office-avatar-desk-top"
        d="M18 77 L74 77 L84 93 L8 93 Z"
      />
      <path
        className="office-avatar-desk-inlay"
        d="M21 79 L71 79 L79 91 L13 91 Z"
      />
      <path
        className="office-avatar-desk-side"
        d="M8 93 L84 93 L84 100 L8 100 Z"
      />
      <path
        className="office-avatar-desk-side office-avatar-desk-side-right"
        d="M84 93 L90 90 L90 97 L84 100 Z"
      />
      <path className="office-avatar-desk-edge" d="M10 94 H82" />
      <ellipse
        className="office-avatar-desk-spill"
        cx="20"
        cy="87"
        rx="12"
        ry="3.4"
      />
      <ellipse
        className="office-avatar-desk-spill office-avatar-desk-spill-warm"
        cx="72"
        cy="87"
        rx="12"
        ry="3.4"
      />
      <path className="office-avatar-desk-leg" d="M17 100 V105 M75 100 V105" />
      <path className="office-avatar-desk-drawer" d="M62 96 H78 V103 H62 Z" />
      <path className="office-avatar-desk-drawer-line" d="M64 98.5 H76" />
      <circle
        className="office-avatar-desk-drawer-handle"
        cx="70"
        cy="100.8"
        r="1"
      />
      <ellipse
        className="office-avatar-hand office-avatar-hand-desk"
        cx="36"
        cy="83"
        rx="3"
        ry="2.2"
      />
      <ellipse
        className="office-avatar-hand office-avatar-hand-desk"
        cx="57"
        cy="83"
        rx="3"
        ry="2.2"
      />
      <path
        className="office-avatar-keyboard"
        d="M35 84 L57 84 L61 90 L31 90 Z"
      />
      <path className="office-avatar-keyboard-keys" d="M36 86 H56 M34 88 H58" />
      <rect
        className="office-avatar-mug"
        x="68"
        y="82"
        width="5"
        height="5.2"
        rx="1.2"
      />
      <path
        className="office-avatar-mug-handle"
        d="M73 83.4 Q76 84.8 73 86.2"
      />
      <rect
        className="office-avatar-notebook"
        x="16"
        y="83"
        width="11"
        height="4"
        rx="1"
        transform="rotate(-4 16 83)"
      />
      <path
        className="office-avatar-desk-cable"
        d="M30 77 Q34 80 36 84 M64 77 Q60 80 58 84"
      />
      {role === "LEAD" && (
        <g className="office-avatar-desk-phone">
          <path d="M76 84 L82 82 L84 86 L78 88 Z" />
          <path d="M78 86 L80 82" />
        </g>
      )}
      {role === "REVIEWER" && (
        <g className="office-avatar-laptop">
          <path d="M22 80 L34 80 L35 87 L21 87 Z" />
          <path d="M23 81 L33 81 L34 85 L22 85 Z" />
          <path d="M21 87 H35" />
        </g>
      )}
      {role === "VERIFIER" && (
        <g className="office-avatar-test-device">
          <rect x="76" y="79" width="8" height="5" rx="1" />
          <circle cx="78" cy="81" r="1" />
          <circle cx="81" cy="81" r="1" />
        </g>
      )}
      {role === "WORKER" && (
        <g className="office-avatar-rack-unit">
          <rect x="75" y="76" width="10" height="4" rx="1" />
          <path d="M77 78 H83" />
        </g>
      )}
      {look.accessory >= 2 && (
        <path className="office-avatar-desk-cable" d="M28 76 Q32 79 34 82" />
      )}
    </g>
  );
}

export function SeatedFigure({
  look,
  role,
}: {
  look: OfficeAvatarLook;
  role: ParticipantRole;
}) {
  return (
    <>
      <ellipse
        className="office-avatar-shadow"
        cx="46"
        cy="107"
        rx="35"
        ry="4.2"
      />
      <Chair look={look} />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M39 78 L35 99"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M53 78 L57 99"
      />
      <path className="office-avatar-shoe" d="M30 102 H41" />
      <path className="office-avatar-shoe" d="M51 102 H62" />
      <TorsoAndArms look={look} seated />
      <rect
        className="office-avatar-neck"
        x="42.3"
        y="35"
        width="7.4"
        height="10"
        rx="3"
      />
      <g transform={`rotate(${((look.hairStyle % 3) - 1) * 3.2} 46 36)`}>
        <SeatedHead look={look} />
      </g>
      <g className="office-avatar-monitor-glow-group">
        <ellipse
          className="office-avatar-monitor-glow"
          cx="18"
          cy="64"
          rx="18"
          ry="12"
        />
        <ellipse
          className="office-avatar-monitor-glow"
          cx="74"
          cy="64"
          rx="18"
          ry="12"
        />
      </g>
      <MonitorBack x={2} y={47} width={29} height={22} />
      <MonitorBack
        x={61}
        y={47}
        width={29}
        height={22}
        warm={role === "LEAD"}
      />
      <WorkstationDesk role={role} look={look} />
    </>
  );
}

export function StandingFigure({
  look,
  observation,
}: {
  look: OfficeAvatarLook;
  observation: string;
}) {
  return (
    <>
      <ellipse
        className="office-avatar-shadow"
        cx="46"
        cy="107"
        rx="20"
        ry="3.8"
      />
      <ellipse
        className="office-avatar-idle-ring"
        data-observation={observation}
        cx="46"
        cy="106"
        rx="26"
        ry="7.6"
      />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M42 77 L38 102"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M51 77 L56 102"
      />
      <path className="office-avatar-shoe" d="M33 105 H42" />
      <path className="office-avatar-shoe" d="M52 105 H61" />
      <TorsoAndArms look={look} seated={false} />
      <rect
        className="office-avatar-neck"
        x="42.3"
        y="35"
        width="7.4"
        height="10"
        rx="3"
      />
      <Head look={look} />
      <Badge seated={false} />
    </>
  );
}
