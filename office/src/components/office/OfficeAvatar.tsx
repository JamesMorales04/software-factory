import type { CSSProperties, KeyboardEvent } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../ui/cn";
import {
  type OfficeAvatarLook,
  officeAvatarLook,
} from "./officeAvatarAppearance";
import {
  OFFICE_AVATAR_HEIGHT,
  OFFICE_AVATAR_WIDTH,
  type OfficeSlot,
} from "./officeSceneLayout";
import { observationLabel, roleLabel } from "./participantLabels";

const OBSERVATION_GLYPH: Record<string, string> = {
  CURRENT: "●",
  STALE: "◐",
  FINAL: "■",
  NOT_EXPOSED: "◇",
  UNKNOWN: "?",
};

function identityLabel(value: string): string {
  if (value.length <= 10) return value;
  return `…${value.slice(-8)}`;
}

function focusNeighbor(
  slot: OfficeSlot,
  key: string,
  slots: OfficeSlot[],
  refs: Map<string, HTMLButtonElement>,
) {
  const candidates = slots
    .filter(
      (candidate) =>
        candidate.participant.participant_id !==
        slot.participant.participant_id,
    )
    .map((candidate) => ({
      candidate,
      dx: candidate.anchorX - slot.anchorX,
      dy: candidate.anchorY - slot.anchorY,
    }))
    .filter(({ dx, dy }) => {
      if (key === "ArrowRight") return dx > 0 && Math.abs(dy) <= Math.abs(dx);
      if (key === "ArrowLeft") return dx < 0 && Math.abs(dy) <= Math.abs(dx);
      if (key === "ArrowDown") return dy > 0 && Math.abs(dx) <= Math.abs(dy);
      if (key === "ArrowUp") return dy < 0 && Math.abs(dx) <= Math.abs(dy);
      return false;
    })
    .sort(
      (left, right) =>
        Math.hypot(left.dx, left.dy) - Math.hypot(right.dx, right.dy),
    );
  const next = candidates[0]?.candidate;
  if (next) refs.get(next.participant.participant_id)?.focus();
}

function lookVariables(look: OfficeAvatarLook): CSSProperties {
  return {
    "--office-skin": `var(--office-skin-${look.skin})`,
    "--office-hair": `var(--office-hair-${look.hairColor})`,
    "--office-shirt": `var(--office-shirt-${look.clothing})`,
    "--office-accent": `var(--office-accent-${look.accent})`,
  } as CSSProperties;
}

function Hair({ look }: { look: OfficeAvatarLook }) {
  switch (look.hairStyle) {
    case 1:
      return (
        <path
          className="office-avatar-hair"
          d="M33.4 26 Q33 13 42 13 Q51 13 50.6 26 Q47 18 42 19 Q36.6 20 33.4 26 Z"
        />
      );
    case 2:
      return (
        <path
          className="office-avatar-hair"
          d="M32.8 28 Q32 13 42 13 Q52 13 51.2 28 L50 39 Q47 37 47 30 Q42 33 37 30 Q37 37 34 39 Z"
        />
      );
    case 3:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M34 26 Q34 13 42 13 Q50 13 50 26 Q47 19 42 19 Q37 19 34 26 Z"
          />
          <circle className="office-avatar-hair" cx="42" cy="12.5" r="4.3" />
          <circle className="office-avatar-hair-tie" cx="42" cy="16" r="1.2" />
        </>
      );
    case 4:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M33 27 Q33 13 42 13 Q51 13 51 27 Q47 19 42 19 Q37 19 33 27 Z"
          />
          <circle className="office-avatar-hair" cx="34" cy="22" r="3.2" />
          <circle className="office-avatar-hair" cx="50" cy="22" r="3.2" />
          <circle className="office-avatar-hair" cx="37" cy="16" r="3" />
          <circle className="office-avatar-hair" cx="47" cy="16" r="3" />
        </>
      );
    case 5:
      return (
        <>
          <path
            className="office-avatar-hair"
            d="M33.4 27 Q33 13 42 13 Q51 13 50.6 27 Q47 19 42 19 Q36.8 20 33.4 27 Z"
          />
          <path
            className="office-avatar-hair"
            d="M49 19 Q56 23 53 35 Q51 39 48 37 Q52 30 48 22 Z"
          />
        </>
      );
    default:
      return (
        <path
          className="office-avatar-hair"
          d="M34 27 Q34 13 42 13 Q50 13 50 27 Q47 19 42 19 Q37 19 34 27 Z"
        />
      );
  }
}

function Accessory({ look }: { look: OfficeAvatarLook }) {
  if (look.accessory === 1) {
    return (
      <g className="office-avatar-glasses">
        <rect x="36.4" y="26.6" width="4.4" height="3.4" rx="1.2" />
        <rect x="43.2" y="26.6" width="4.4" height="3.4" rx="1.2" />
        <path d="M40.8 27.8 H43.2" />
      </g>
    );
  }
  if (look.accessory === 2) {
    return (
      <g className="office-avatar-headphones">
        <path d="M33.4 28 Q33 15 42 14 Q51 15 50.6 28" />
        <rect x="31.2" y="25.6" width="3.4" height="6.2" rx="1.6" />
        <rect x="49.4" y="25.6" width="3.4" height="6.2" rx="1.6" />
      </g>
    );
  }
  return null;
}

function Face() {
  return (
    <g className="office-avatar-face">
      <ellipse
        className="office-avatar-ear"
        cx="34.6"
        cy="29"
        rx="1.5"
        ry="2"
      />
      <ellipse
        className="office-avatar-ear"
        cx="49.4"
        cy="29"
        rx="1.5"
        ry="2"
      />
      <path className="office-avatar-brow" d="M37.4 26.2 q2 -1 3.8 0" />
      <path className="office-avatar-brow" d="M42.8 26.2 q2 -1 3.8 0" />
      <circle className="office-avatar-eye" cx="39.2" cy="28" r="0.8" />
      <circle className="office-avatar-eye" cx="44.8" cy="28" r="0.8" />
      <path className="office-avatar-nose" d="M42 29.2 v2.2" />
      <path className="office-avatar-mouth" d="M40.4 33.2 q1.6 1.2 3.2 0" />
    </g>
  );
}

function SeatedFigure({ look }: { look: OfficeAvatarLook }) {
  return (
    <>
      <ellipse
        className="office-avatar-shadow"
        cx="42"
        cy="93"
        rx="30"
        ry="4"
      />
      <rect
        className="office-avatar-chair"
        x="27"
        y="48"
        width="30"
        height="34"
        rx="8"
      />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M36 71 L34 91"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M48 71 L50 91"
      />
      <path className="office-avatar-shoe" d="M29 92 H38" />
      <path className="office-avatar-shoe" d="M46 92 H55" />
      <path
        className="office-avatar-torso"
        d="M27 49 Q42 41 57 49 L60 72 Q42 79 24 72 Z"
      />
      <path
        className="office-avatar-shirt-detail"
        d="M42 47 L39 63 L42 69 L45 63 Z"
      />
      <path
        className="office-avatar-arm office-avatar-arm-left"
        d="M28 53 Q22 62 27 71"
      />
      <path
        className="office-avatar-arm office-avatar-arm-right"
        d="M56 53 Q62 62 57 71"
      />
      <circle className="office-avatar-hand" cx="27" cy="72" r="3" />
      <circle className="office-avatar-hand" cx="57" cy="72" r="3" />
      <rect
        className="office-avatar-neck"
        x="39"
        y="38"
        width="6"
        height="8"
        rx="2.5"
      />
      <ellipse
        className="office-avatar-head"
        cx="42"
        cy="27"
        rx="7.2"
        ry="8.7"
      />
      <Hair look={look} />
      <Face />
      <Accessory look={look} />
      <path className="office-avatar-lanyard" d="M39 47 Q42 55 45 47" />
      <rect
        className="office-avatar-badge"
        x="45.5"
        y="55"
        width="3.2"
        height="4"
        rx="0.6"
      />
      <g className="office-avatar-workstation">
        <ellipse
          className="office-avatar-monitor-glow"
          cx="21"
          cy="63"
          rx="11"
          ry="8"
        />
        <ellipse
          className="office-avatar-monitor-glow"
          cx="63"
          cy="63"
          rx="11"
          ry="8"
        />
        <rect
          className="office-avatar-monitor"
          x="12"
          y="54"
          width="18"
          height="12"
          rx="1.5"
        />
        <rect
          className="office-avatar-screen"
          x="14"
          y="56"
          width="14"
          height="8"
          rx="0.8"
        />
        <path className="office-avatar-screen-line" d="M16 58 H24 M16 60 H22" />
        <rect
          className="office-avatar-monitor"
          x="54"
          y="54"
          width="18"
          height="12"
          rx="1.5"
        />
        <rect
          className="office-avatar-screen"
          x="56"
          y="56"
          width="14"
          height="8"
          rx="0.8"
        />
        <path className="office-avatar-screen-line" d="M58 58 H66 M58 60 H64" />
        <path
          className="office-avatar-monitor-stand"
          d="M21 66 V71 M63 66 V71"
        />
        <path
          className="office-avatar-desk-top"
          d="M5 78 L42 68 L79 78 L42 91 Z"
        />
        <path
          className="office-avatar-desk-side"
          d="M5 78 L42 91 L42 97 L5 84 Z"
        />
        <path
          className="office-avatar-desk-side office-avatar-desk-side-right"
          d="M42 91 L79 78 L79 84 L42 97 Z"
        />
        <rect
          className="office-avatar-keyboard"
          x="33"
          y="79"
          width="18"
          height="3"
          rx="1"
        />
        <rect
          className="office-avatar-mug"
          x="68"
          y="76"
          width="4"
          height="4"
          rx="1"
        />
      </g>
    </>
  );
}

function StandingFigure({
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
        cx="42"
        cy="94"
        rx="18"
        ry="3.5"
      />
      <ellipse
        className="office-avatar-idle-ring"
        data-observation={observation}
        cx="42"
        cy="93"
        rx="23"
        ry="7"
      />
      <path
        className="office-avatar-leg office-avatar-leg-left"
        d="M37 67 L35 91"
      />
      <path
        className="office-avatar-leg office-avatar-leg-right"
        d="M47 67 L49 91"
      />
      <path className="office-avatar-shoe" d="M30 92 H38" />
      <path className="office-avatar-shoe" d="M46 92 H54" />
      <path
        className="office-avatar-torso"
        d="M29 43 Q42 38 55 43 L57 68 Q42 73 27 68 Z"
      />
      <path
        className="office-avatar-shirt-detail"
        d="M42 41 L39 55 L42 61 L45 55 Z"
      />
      <path className="office-avatar-arm" d="M29 47 Q25 57 27 66" />
      <path className="office-avatar-arm" d="M55 47 Q59 57 57 66" />
      <circle className="office-avatar-hand" cx="27" cy="67" r="3" />
      <circle className="office-avatar-hand" cx="57" cy="67" r="3" />
      <rect
        className="office-avatar-neck"
        x="39"
        y="38"
        width="6"
        height="8"
        rx="2.5"
      />
      <ellipse
        className="office-avatar-head"
        cx="42"
        cy="27"
        rx="7.2"
        ry="8.7"
      />
      <Hair look={look} />
      <Face />
      <Accessory look={look} />
      <path className="office-avatar-lanyard" d="M39 42 Q42 50 45 42" />
      <rect
        className="office-avatar-badge"
        x="45.5"
        y="50"
        width="3.2"
        height="4"
        rx="0.6"
      />
    </>
  );
}

export function OfficeAvatar({
  slot,
  offsetX,
  offsetY,
  selected,
  onSelect,
  registerRef,
  slots,
  refs,
}: {
  slot: OfficeSlot;
  offsetX: number;
  offsetY: number;
  selected: boolean;
  onSelect: (participantId: string) => void;
  registerRef: (
    participantId: string,
    element: HTMLButtonElement | null,
  ) => void;
  slots: OfficeSlot[];
  refs: Map<string, HTMLButtonElement>;
}) {
  const { t } = useI18n();
  const { participant } = slot;
  const look = officeAvatarLook(slot.lookIndex);
  const observation = observationLabel(participant);
  const glyph = OBSERVATION_GLYPH[participant.observation] ?? "?";
  const role = roleLabel(participant.role, t);
  const working = participant.observation === "CURRENT";

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (
      ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"].includes(event.key)
    ) {
      event.preventDefault();
      focusNeighbor(slot, event.key, slots, refs);
    }
  }

  return (
    <button
      ref={(element) => registerRef(participant.participant_id, element)}
      type="button"
      className={cn("office-avatar", selected && "is-selected")}
      style={
        {
          ...lookVariables(look),
          left: slot.x - offsetX,
          top: slot.y - offsetY,
          width: OFFICE_AVATAR_WIDTH,
          height: OFFICE_AVATAR_HEIGHT,
          "--office-depth": 10 + slot.depth,
        } as CSSProperties
      }
      aria-pressed={selected}
      aria-label={t("agentOffice.card.inspect", {
        role,
        id: participant.participant_id,
        status: participant.status,
        observation,
      })}
      onClick={() => onSelect(participant.participant_id)}
      onKeyDown={onKeyDown}
      data-office-avatar=""
      data-testid={`office-avatar-${participant.participant_id}`}
      data-participant-id={participant.participant_id}
      data-role={participant.role}
      data-observation={participant.observation}
      data-working={working ? "true" : "false"}
      data-look={look.signature}
      data-slot-index={slot.index}
      data-column={slot.column}
      data-row={slot.row}
      data-anchor-x={Math.round(slot.anchorX)}
      data-anchor-y={Math.round(slot.anchorY)}
    >
      <span className="office-avatar-card" aria-hidden="true">
        <span className="office-avatar-card-role">{role}</span>
        <span className="office-avatar-card-identity">
          <code>{identityLabel(participant.identity_id)}</code>
        </span>
        <span
          className="office-avatar-card-status"
          data-observation={participant.observation}
        >
          <span className="office-avatar-card-glyph">{glyph}</span>
          {participant.status} · {observation}
        </span>
      </span>
      <svg
        className="office-avatar-figure"
        viewBox="0 0 84 100"
        aria-hidden="true"
        focusable="false"
      >
        {working ? (
          <SeatedFigure look={look} />
        ) : (
          <StandingFigure look={look} observation={participant.observation} />
        )}
        <circle
          className="office-avatar-status-lamp"
          data-observation={participant.observation}
          cx="74"
          cy="90"
          r="2.6"
        />
      </svg>
    </button>
  );
}
