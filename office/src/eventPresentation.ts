import type { FactoryEvent } from "./api";

const EVENT_LABELS: Record<string, string> = {
  CoreStarted: "Core started",
  RunQueued: "Run queued",
  RunLeased: "Worker leased the Run",
  RunStarted: "Attempt started",
  RunCompleted: "Run completed",
  RunFailed: "Run failed",
  RunExpired: "Attempt expired",
  RunCancelled: "Run cancelled",
  WorkerRegistered: "Worker registered",
  WorkerOffline: "Worker offline",
  PaseoSessionIntended: "Paseo session intended",
  PaseoSessionActive: "Paseo session active",
  PaseoSessionSucceeded: "Paseo session succeeded",
  PaseoSessionFailed: "Paseo session failed",
  PaseoSessionCancelled: "Paseo session cancelled",
  PaseoSessionOrphaned: "Paseo session orphaned",
  ActionCreated: "Action created",
  ActionTransitioned: "Action transitioned",
};

function text(value: unknown, max = 160): string {
  if (typeof value !== "string") return "";
  const clean = value.replace(/\s+/g, " ").trim();
  return clean.length > max ? `${clean.slice(0, max)}…` : clean;
}

function attempt(payload: Record<string, unknown>): string {
  const seq = typeof payload.seq === "number" ? `seq ${payload.seq}` : "";
  const generation =
    typeof payload.generation === "number"
      ? `generation ${payload.generation}`
      : "";
  return [seq, generation].filter(Boolean).join(" · ");
}

/**
 * Bounded, whitelisted event presentation. Event payloads may contain
 * operator/Worker evidence (for example a completed Run's output); the UI
 * must never dump arbitrary payload JSON. Only fields named here are shown.
 */
export function presentEvent(event: FactoryEvent): {
  label: string;
  detail: string;
} {
  const p = event.payload ?? {};
  const parts: string[] = [];
  let label = EVENT_LABELS[event.eventType] ?? event.eventType;
  if (event.eventType === "RunFailed" && p.willRetry === true) {
    label = "Attempt failed — retry queued";
  }

  switch (event.eventType) {
    case "RunQueued":
      parts.push(text(p.workloadKind), text(p.executionMode));
      break;
    case "RunLeased":
      parts.push(attempt(p), text(p.workerId));
      break;
    case "RunStarted":
      parts.push(attempt(p), text(p.workerId));
      break;
    case "RunCompleted":
    case "RunFailed":
    case "RunExpired":
      parts.push(attempt(p), text(p.workerId), text(p.error), text(p.reason));
      break;
    case "RunCancelled":
      parts.push(attempt(p), text(p.workerId));
      break;
    case "WorkerRegistered":
    case "WorkerOffline":
      parts.push(text(p.reason), text(p.version));
      break;
    case "PaseoSessionIntended":
      parts.push(attempt(p), text(p.workerId), text(p.provider), text(p.model));
      parts.push(text(p.mode));
      break;
    case "PaseoSessionActive":
      parts.push(attempt(p), text(p.workspaceId), text(p.sessionId));
      parts.push(text(p.provider), text(p.model));
      break;
    case "ActionCreated":
      parts.push(text(p.projectId), text(p.status));
      break;
    case "ActionTransitioned":
      parts.push(text(p.from), text(p.to));
      break;
    case "PaseoSessionSucceeded":
    case "PaseoSessionFailed":
    case "PaseoSessionCancelled":
    case "PaseoSessionOrphaned":
      parts.push(attempt(p), text(p.sessionId), text(p.error), text(p.reason));
      break;
    default:
      break;
  }

  return {
    label,
    detail: parts.filter(Boolean).join(" · "),
  };
}
