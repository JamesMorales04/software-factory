export const SESSION_STATUSES = [
  "intended",
  "active",
  "succeeded",
  "failed",
  "cancelled",
  "orphaned",
] as const;

export const SESSION_PROVIDERS = ["codex", "opencode"] as const;

const NON_TERMINAL = new Set(["intended", "active"]);

export function isLiveSession(status: string): boolean {
  return NON_TERMINAL.has(status);
}

export function isOrphanedSession(status: string): boolean {
  return status === "orphaned";
}

export function shortId(id: string | undefined, length = 8): string {
  if (!id) return "—";
  return id.length > length ? id.slice(0, length) : id;
}

export function formatTimestamp(value: string | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString();
}

export function fullTimestamp(value: string | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toISOString();
}

export function stringField(
  source: Record<string, unknown> | undefined,
  key: string,
): string | null {
  const value = source?.[key];
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

export function numberField(
  source: Record<string, unknown> | undefined,
  key: string,
): number | null {
  const value = source?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function exitLabel(value: number | null): string {
  if (value === null) return "not recorded";
  if (value < 0) return "not applicable";
  return `${value === 0 ? "pass" : "fail"} (exit ${value})`;
}
