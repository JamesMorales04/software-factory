import { Badge, type BadgeTone } from "../components/ui/Badge";
import type { IntakeMode, IntakeState } from "../coreApi";
import { useI18n } from "../i18n";

const STATE_KEYS = {
  SUBMITTED: "request.state.SUBMITTED",
  ASSESSING: "request.state.ASSESSING",
  PLAN_READY: "request.state.PLAN_READY",
  AWAITING_APPROVAL: "request.state.AWAITING_APPROVAL",
  EXECUTING: "request.state.EXECUTING",
  BLOCKED: "request.state.BLOCKED",
  FAILED: "request.state.FAILED",
  COMPLETED: "request.state.COMPLETED",
  CANCELLED: "request.state.CANCELLED",
} as const satisfies Record<IntakeState, string>;

const STATE_TONES: Record<IntakeState, BadgeTone> = {
  SUBMITTED: "neutral",
  ASSESSING: "info",
  PLAN_READY: "info",
  AWAITING_APPROVAL: "attention",
  EXECUTING: "accent",
  BLOCKED: "danger",
  FAILED: "danger",
  COMPLETED: "success",
  CANCELLED: "muted",
};

const MODE_KEYS = {
  REVIEW_PLAN: "request.mode.REVIEW_PLAN",
  AUTONOMOUS: "request.mode.AUTONOMOUS",
} as const satisfies Record<IntakeMode, string>;

export function intakeStateTone(state: IntakeState | string): BadgeTone {
  if (state in STATE_TONES) return STATE_TONES[state as IntakeState];
  return "muted";
}

export function IntakeStateBadge({ state }: { state: IntakeState | string }) {
  const { t } = useI18n();
  const known = state in STATE_KEYS ? STATE_KEYS[state as IntakeState] : null;
  const label = known ? t(known) : state.replaceAll("_", " ").toLowerCase();
  return (
    <Badge tone={intakeStateTone(state)} className="request-state-badge">
      {label}
    </Badge>
  );
}

export function IntakeModeBadge({ mode }: { mode: IntakeMode | string }) {
  const { t } = useI18n();
  const known = mode in MODE_KEYS ? MODE_KEYS[mode as IntakeMode] : null;
  const label = known ? t(known) : mode.replaceAll("_", " ").toLowerCase();
  return (
    <Badge tone={mode === "AUTONOMOUS" ? "attention" : "muted"}>{label}</Badge>
  );
}

export function isTerminalIntakeState(state: IntakeState | string): boolean {
  return state === "COMPLETED" || state === "CANCELLED";
}
