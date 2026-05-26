export function formatDate(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function pretty(value: unknown): string {
  return JSON.stringify(value ?? {}, null, 2);
}
