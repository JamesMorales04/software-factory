import type { OfficeAvatarLook } from "./officeAvatarAppearance";

/**
 * Original 3/4-view figure geometry for the Agent Office.
 *
 * The figure is drawn in a 92 × 112 local coordinate space. It has no
 * animation, pose change, tool use or progress: a CURRENT observation uses the
 * seated studio pose, every other observation uses the standing pose with an
 * observation ring. Appearance is derived only from the stable look index.
 */

type HairProps = { look: OfficeAvatarLook };

function HairBack({ look }: HairProps) {
  switch (look.hairStyle) {
    case 2:
      return (
        <path
          className="office-avatar-hair office-avatar-hair-back"
          d="M35 25 Q35 12 46 12 Q57 12 57 25 L57 38 Q54 41 52 37 L52 24 Q49 20 46 20 Q43 20 40 24 L40 37 Q38 41 35 38 Z"
        />
      );
    case 3:
      return (
        <>
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M36 26 Q36 13 46 13 Q56 13 56 26 L55 31 L37 31 Z"
          />
          <circle
            className="office-avatar-hair office-avatar-hair-back"
            cx="56"
            cy="16"
            r="5.2"
          />
          <circle className="office-avatar-hair-tie" cx="56" cy="20" r="1.2" />
        </>
      );
    case 4:
      return (
        <g className="office-avatar-hair office-avatar-hair-back">
          <circle cx="37" cy="22" r="4.2" />
          <circle cx="55" cy="22" r="4.2" />
          <circle cx="40" cy="15" r="4.4" />
          <circle cx="52" cy="15" r="4.4" />
          <circle cx="46" cy="13" r="4.6" />
          <circle cx="35" cy="28" r="3.2" />
          <circle cx="57" cy="28" r="3.2" />
        </g>
      );
    case 5:
      return (
        <>
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M36 26 Q36 13 46 13 Q56 13 56 26 L54 30 L38 30 Z"
          />
          <path
            className="office-avatar-hair office-avatar-hair-back"
            d="M54 18 Q62 21 61 31 Q60 37 57 35 Q60 28 55 22 Z"
          />
        </>
      );
    default:
      return (
        <path
          className="office-avatar-hair office-avatar-hair-back"
          d="M37 26 Q37 14 46 14 Q55 14 55 26 L53 31 L39 31 Z"
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
    </g>
  );
}

function Head({ look }: HairProps) {
  return (
    <>
      <HairBack look={look} />
      <ellipse
        className="office-avatar-head"
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
      <Accessory look={look} />
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
  const width = look.build === 1 ? 1 : look.build === 3 ? -2 : 0;
  const shoulders = 28 + width;
  const shouldersRight = 64 - width;
  const torso = seated
    ? `M${shoulders} 50 Q46 40 ${shouldersRight} 50 L${66 - width} 73 Q46 80 ${26 + width} 73 Z`
    : `M${shoulders} 46 Q46 38 ${shouldersRight} 46 L${64 - width} 76 Q46 81 ${28 + width} 76 Z`;
  const leftArm = seated
    ? `M${shoulders + 2} 54 Q${20 + width} 64 ${28 + width} 74`
    : `M${shoulders + 2} 50 Q${22 + width} 62 ${26 + width} 74`;
  const rightArm = seated
    ? `M${shouldersRight - 2} 54 Q${72 - width} 64 ${64 - width} 74`
    : `M${shouldersRight - 2} 50 Q${70 - width} 62 ${66 - width} 74`;
  return (
    <>
      <path className="office-avatar-torso" d={torso} />
      {look.jacket > 0 && (
        <g className="office-avatar-jacket">
          <path
            d={`M${shoulders + 2} 50 Q36 44 42 49 L40 74 L${26 + width} 73 Z`}
          />
          <path
            d={`M${shouldersRight - 2} 50 Q56 44 50 49 L52 74 L${66 - width} 73 Z`}
          />
        </g>
      )}
      <path
        className="office-avatar-shirt-detail"
        d={
          seated
            ? "M46 45 L43 62 L46 68 L49 62 Z"
            : "M46 41 L43 58 L46 65 L49 58 Z"
        }
      />
      <path className="office-avatar-arm office-avatar-arm-left" d={leftArm} />
      <path
        className="office-avatar-arm office-avatar-arm-right"
        d={rightArm}
      />
      <path
        className="office-avatar-cuff"
        d={seated ? `M${27 + width} 70.2 l3 4.6` : `M${25 + width} 71 l2.6 4.4`}
      />
      <path
        className="office-avatar-cuff"
        d={
          seated ? `M${65 - width} 70.2 l-3 4.6` : `M${67 - width} 71 l-2.6 4.4`
        }
      />
      <circle
        className="office-avatar-hand"
        cx={28 + width}
        cy={seated ? 75 : 76}
        r="3.1"
      />
      <circle
        className="office-avatar-hand"
        cx={64 - width}
        cy={seated ? 75 : 76}
        r="3.1"
      />
    </>
  );
}

function Badge({ seated }: { seated: boolean }) {
  const top = seated ? 51 : 47;
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
      <rect
        className="office-avatar-chair"
        x="29"
        y="42"
        width="34"
        height="34"
        rx="11"
      />
      <path
        className="office-avatar-chair-seam"
        d="M33 50 H59 M33 56 H59 M34 62 H58"
      />
      <path className="office-avatar-chair-post" d="M45 97 V103" />
      <path className="office-avatar-chair-base" d="M34 106 L45 101 L58 106" />
      <circle className="office-avatar-wheel" cx="33" cy="107" r="2.2" />
      <circle className="office-avatar-wheel" cx="45" cy="108.4" r="2.2" />
      <circle className="office-avatar-wheel" cx="59" cy="107" r="2.2" />
      <path className="office-avatar-chair-arm" d="M27 55 Q25 63 29 69" />
      <path className="office-avatar-chair-arm" d="M65 55 Q67 63 63 69" />
      <circle className="office-avatar-chair-arm-cap" cx="29" cy="69" r="2" />
      <circle className="office-avatar-chair-arm-cap" cx="63" cy="69" r="2" />
      <path
        className="office-avatar-chair-stem-colour"
        d="M30 45 Q32 38 46 38 Q60 38 62 45"
      />
      <circle className="office-avatar-chair-logo" cx="46" cy="60" r="1.6" />
      {look.jacket > 0 && (
        <path className="office-avatar-chair-accent" d="M31 46 H61" />
      )}
    </g>
  );
}

export function SeatedFigure({ look }: { look: OfficeAvatarLook }) {
  return (
    <>
      <ellipse
        className="office-avatar-shadow"
        cx="46"
        cy="105"
        rx="34"
        ry="4"
      />
      <Chair look={look} />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M40 76 L35 98"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M53 76 L58 98"
      />
      <path className="office-avatar-shoe" d="M29 101 H40" />
      <path className="office-avatar-shoe" d="M53 101 H64" />
      <TorsoAndArms look={look} seated />
      <rect
        className="office-avatar-neck"
        x="42.3"
        y="35"
        width="7.4"
        height="10"
        rx="3"
      />
      <Head look={look} />
      <Badge seated />
      <g className="office-avatar-workstation">
        <ellipse
          className="office-avatar-monitor-glow"
          cx="21"
          cy="63"
          rx="13"
          ry="9"
        />
        <ellipse
          className="office-avatar-monitor-glow"
          cx="71"
          cy="63"
          rx="13"
          ry="9"
        />
        <rect
          className="office-avatar-monitor"
          x="10"
          y="53"
          width="22"
          height="15"
          rx="1.8"
        />
        <rect
          className="office-avatar-screen"
          x="12"
          y="55"
          width="18"
          height="11"
          rx="0.8"
        />
        <path
          className="office-avatar-screen-line"
          d="M14 58 H25 M14 60.5 H23 M14 63 H21"
        />
        <rect
          className="office-avatar-monitor"
          x="60"
          y="53"
          width="22"
          height="15"
          rx="1.8"
        />
        <rect
          className="office-avatar-screen"
          x="62"
          y="55"
          width="18"
          height="11"
          rx="0.8"
        />
        <path
          className="office-avatar-screen-line"
          d="M64 58 H75 M64 60.5 H73 M64 63 H71"
        />
        <path
          className="office-avatar-monitor-stand"
          d="M21 68 V73 M71 68 V73"
        />
        <path
          className="office-avatar-desk-top"
          d="M4 79 L46 68 L88 79 L46 93 Z"
        />
        <path
          className="office-avatar-desk-side"
          d="M4 79 L46 93 L46 100 L4 86 Z"
        />
        <path
          className="office-avatar-desk-side office-avatar-desk-side-right"
          d="M46 93 L88 79 L88 86 L46 100 Z"
        />
        <path className="office-avatar-desk-edge" d="M10 80 L46 70 L82 80" />
        <rect
          className="office-avatar-keyboard"
          x="33"
          y="82"
          width="24"
          height="4.2"
          rx="1"
        />
        <path className="office-avatar-keyboard-keys" d="M36 83.4 H54" />
        <rect
          className="office-avatar-mug"
          x="70"
          y="78"
          width="4.6"
          height="4.6"
          rx="1"
        />
        <path
          className="office-avatar-mug-handle"
          d="M74.6 79.4 Q77 80.6 74.6 81.8"
        />
        <rect
          className="office-avatar-notebook"
          x="15"
          y="82"
          width="9"
          height="3.4"
          rx="0.8"
        />
        {look.accessory >= 2 && (
          <path
            className="office-avatar-desk-cable"
            d="M27 72 Q32 74 34 78 M64 72 Q60 74 58 78"
          />
        )}
      </g>
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
        rx="19"
        ry="3.6"
      />
      <ellipse
        className="office-avatar-idle-ring"
        data-observation={observation}
        cx="46"
        cy="106"
        rx="25"
        ry="7.5"
      />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M42 76 L38 102"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M51 76 L55 102"
      />
      <path className="office-avatar-shoe" d="M33 105 H42" />
      <path className="office-avatar-shoe" d="M51 105 H60" />
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
