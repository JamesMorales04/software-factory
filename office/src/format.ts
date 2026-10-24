export function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

export function shortId(value: string, length = 8): string {
  return value.length > length ? value.slice(0, length) : value;
}
