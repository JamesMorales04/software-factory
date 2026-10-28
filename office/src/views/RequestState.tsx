import { Badge, type BadgeTone } from "../components/ui/Badge";
import type { IntakeMode, IntakeState } from "../coreApi";

const STATE_LABELS: Record<IntakeState, string> = {
  SUBMITTED: "Submitted",
  ASSESSING: "Assessing",
  PLAN_READY: "Plan ready",
  AWAITING_APPROVAL: "Awaiting approval",
  EXECUTING: "Executing",
  BLOCKED: "Blocked",
  FAILED: "Failed",
  COMPLETED: "Completed",
  CANCELLED: "Cancelled",
};

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

export function intakeStateLabel(state: IntakeState | string): string {
  if (state in STATE_LABELS) return STATE_LABELS[state as IntakeState];
  return state.replaceAll("_", " ").toLowerCase();
}

export function intakeStateTone(state: IntakeState | string): BadgeTone {
  if (state in STATE_TONES) return STATE_TONES[state as IntakeState];
  return "muted";
}

export function IntakeStateBadge({ state }: { state: IntakeState | string }) {
  return (
    <Badge tone={intakeStateTone(state)} className="request-state-badge">
      {intakeStateLabel(state)}
    </Badge>
  );
}

export function intakeModeLabel(mode: IntakeMode | string): string {
  if (mode === "REVIEW_PLAN") return "Review plan";
  if (mode === "AUTONOMOUS") return "Autonomous";
  return mode.replaceAll("_", " ").toLowerCase();
}

export function IntakeModeBadge({ mode }: { mode: IntakeMode | string }) {
  return (
    <Badge tone={mode === "AUTONOMOUS" ? "attention" : "muted"}>
      {intakeModeLabel(mode)}
    </Badge>
  );
}

export function isTerminalIntakeState(state: IntakeState | string): boolean {
  return state === "COMPLETED" || state === "CANCELLED";
}
