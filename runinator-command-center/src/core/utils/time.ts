export interface RelativeTimeOptions {
  now?: number;
  prefix?: string;
  nowLabel?: string;
}

/** Format an RFC 3339 timestamp as a compact, non-negative relative age. */
export function formatRelativeTime(value: string, options: RelativeTimeOptions = {}): string {
  const timestamp = new Date(value).getTime();

  if (Number.isNaN(timestamp)) {
    return value;
  }

  const prefix = options.prefix ?? "";
  const seconds = Math.max(0, Math.round(((options.now ?? Date.now()) - timestamp) / 1_000));

  if (seconds < 60) {
    return options.nowLabel ?? `${prefix}now`;
  }

  const minutes = Math.floor(seconds / 60);

  if (minutes < 60) {
    return `${prefix}${String(minutes)}m ago`;
  }

  const hours = Math.floor(minutes / 60);

  if (hours < 24) {
    return `${prefix}${String(hours)}h ago`;
  }

  return `${prefix}${String(Math.floor(hours / 24))}d ago`;
}
