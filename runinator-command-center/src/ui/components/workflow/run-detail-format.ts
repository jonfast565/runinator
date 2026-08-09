import { pretty } from "../../../core/utils/format";
import { displayValue } from "../../../core/utils/values";

export function formatResultValue(value: unknown): string {
  if (value === undefined || value === null) {
    return "(none)";
  }

  return typeof value === "object" ? pretty(value) : displayValue(value);
}

export function formatRunDuration(startedAt?: string | null, finishedAt?: string | null): string {
  if (!startedAt || !finishedAt) {
    return "";
  }

  const start = Date.parse(startedAt);
  const end = Date.parse(finishedAt);

  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return "";
  }

  const seconds = Math.max(0, Math.round((end - start) / 1000));

  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder === 0
    ? `${String(minutes)}m`
    : `${String(minutes)}m ${String(remainder)}s`;
}

export function shortId(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}
