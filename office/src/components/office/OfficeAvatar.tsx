import type { CSSProperties, KeyboardEvent } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../ui/cn";
import { SeatedFigure, StandingFigure } from "./OfficeAvatarFigure";
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
    "--office-trouser": `var(--office-trouser-${look.trouser})`,
    "--office-jacket-colour": `var(--office-jacket-${look.jacket})`,
    "--office-chair": `var(--office-chair-${look.trouser})`,
  } as CSSProperties;
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
          "--office-perspective-scale": slot.perspectiveScale.toFixed(2),
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
      data-build={look.build}
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
      <span className="office-avatar-selected-ring" aria-hidden="true" />
      <svg
        className="office-avatar-figure"
        viewBox={`0 0 ${OFFICE_AVATAR_WIDTH} ${OFFICE_AVATAR_HEIGHT}`}
        aria-hidden="true"
        focusable="false"
      >
        {working ? (
          <SeatedFigure look={look} role={participant.role} />
        ) : (
          <StandingFigure look={look} observation={participant.observation} />
        )}
        <circle
          className="office-avatar-status-lamp"
          data-observation={participant.observation}
          cx={OFFICE_AVATAR_WIDTH - 11}
          cy={OFFICE_AVATAR_HEIGHT - 12}
          r="2.8"
        />
      </svg>
    </button>
  );
}
