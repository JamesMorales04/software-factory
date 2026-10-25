export function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

export function shortId(value: string, length = 8): string {
  return value.length > length ? value.slice(0, length) : value;
}

export function sessionStateClass(state: string): string {
  if (state === "CLEANED" || state === "COMPLETED")
    return "pill recovery-confirmed";
  if (state === "ORPHANED" || state === "FAILED")
    return "pill recovery-blocked";
  return "pill";
}

export function formatCostMicros(value: number): string {
  return `${value.toLocaleString()}µ`;
}

export function formatObservableLimit(value: number): string {
  return value > 0 ? value.toLocaleString() : "not set";
}
