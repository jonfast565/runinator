//! cron expressions for the trigger editor: split into fields, validate, describe in english, and
//! project the next few occurrences.
//!
//! the backend parses these with `croner` and evaluates them in UTC (`Utc::now`), so everything here
//! works in UTC too — a preview computed in the operator's local zone would be wrong by their offset
//! and only visibly wrong twice a year. the five-field form is what this models; the six-field
//! (seconds) and `@alias` forms croner also accepts stay valid and are edited as raw text, since a
//! field builder for them would imply the editor understands shapes it does not.

export type CronFieldName = "minute" | "hour" | "dayOfMonth" | "month" | "dayOfWeek";

export interface CronFields {
  minute: string;
  hour: string;
  dayOfMonth: string;
  month: string;
  dayOfWeek: string;
}

interface FieldSpec {
  min: number;
  max: number;
  label: string;
  /** three-letter names accepted in place of numbers, indexed from `min`. */
  names?: string[];
}

const MONTH_NAMES = [
  "JAN",
  "FEB",
  "MAR",
  "APR",
  "MAY",
  "JUN",
  "JUL",
  "AUG",
  "SEP",
  "OCT",
  "NOV",
  "DEC",
];

const DAY_NAMES = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

const MONTH_LABELS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

const DAY_LABELS = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

const FIELD_SPECS: Record<CronFieldName, FieldSpec> = {
  minute: { min: 0, max: 59, label: "Minute" },
  hour: { min: 0, max: 23, label: "Hour" },
  dayOfMonth: { min: 1, max: 31, label: "Day of month" },
  month: { min: 1, max: 12, label: "Month", names: MONTH_NAMES },
  dayOfWeek: { min: 0, max: 6, label: "Day of week", names: DAY_NAMES },
};

export const CRON_FIELD_ORDER: CronFieldName[] = [
  "minute",
  "hour",
  "dayOfMonth",
  "month",
  "dayOfWeek",
];

export const EVERY_HOUR = "0 * * * *";

/** the schedules an operator picks by name rather than by writing five fields. */
export const CRON_PRESETS: { id: string; label: string; expression: string }[] = [
  { id: "minutely", label: "Every minute", expression: "* * * * *" },
  { id: "five-minutes", label: "Every 5 minutes", expression: "*/5 * * * *" },
  { id: "fifteen-minutes", label: "Every 15 minutes", expression: "*/15 * * * *" },
  { id: "hourly", label: "Hourly", expression: EVERY_HOUR },
  { id: "daily", label: "Daily at midnight", expression: "0 0 * * *" },
  { id: "weekdays", label: "Weekdays at 09:00", expression: "0 9 * * 1-5" },
  { id: "weekly", label: "Weekly on Sunday", expression: "0 0 * * 0" },
  { id: "monthly", label: "Monthly on the 1st", expression: "0 0 1 * *" },
];

export const CUSTOM_PRESET_ID = "custom";

/** split a five-field expression into its fields; null for anything else (aliases, seconds form). */
export function splitCron(expression: string): CronFields | null {
  const parts = expression.trim().split(/\s+/);

  if (parts.length !== 5) {
    return null;
  }

  const [minute, hour, dayOfMonth, month, dayOfWeek] = parts;
  return { minute, hour, dayOfMonth, month, dayOfWeek };
}

export function joinCron(fields: CronFields): string {
  return CRON_FIELD_ORDER.map((name) => fields[name].trim() || "*").join(" ");
}

export function emptyCronFields(): CronFields {
  return { minute: "0", hour: "*", dayOfMonth: "*", month: "*", dayOfWeek: "*" };
}

/** the preset whose expression this is, or `custom`. */
export function matchCronPreset(expression: string): string {
  const normalized = expression.trim().replace(/\s+/g, " ");
  return (
    CRON_PRESETS.find((preset) => preset.expression === normalized)?.id ?? CUSTOM_PRESET_ID
  );
}

function nameIndex(spec: FieldSpec, token: string): number | null {
  if (!spec.names) {
    return null;
  }

  const index = spec.names.indexOf(token.toUpperCase());
  return index < 0 ? null : index + spec.min;
}

/** resolve one endpoint of a term to a number, accepting the field's three-letter names. */
function endpoint(spec: FieldSpec, token: string, name: CronFieldName): number | null {
  const named = nameIndex(spec, token);

  if (named !== null) {
    return named;
  }

  if (!/^\d+$/.test(token)) {
    return null;
  }

  const value = Number(token);

  // sunday is both 0 and 7 in every cron dialect worth supporting.
  if (name === "dayOfWeek" && value === 7) {
    return 0;
  }

  return value >= spec.min && value <= spec.max ? value : null;
}

/**
 * expand a field into the set of values it matches, or null when it is malformed.
 * handles `*`, `a`, `a-b`, `*\/s`, `a-b/s`, `a/s`, and comma-separated lists of those.
 */
export function expandCronField(field: string, name: CronFieldName): Set<number> | null {
  const spec = FIELD_SPECS[name];
  const trimmed = field.trim();

  if (!trimmed) {
    return null;
  }

  const values = new Set<number>();

  for (const term of trimmed.split(",")) {
    const segments = term.split("/");

    if (segments.length > 2) {
      return null;
    }

    const rangePart = segments[0];
    const stepPart = segments.length === 2 ? segments[1] : "";
    let step = 1;

    if (stepPart !== "") {
      if (!/^\d+$/.test(stepPart) || Number(stepPart) < 1) {
        return null;
      }

      step = Number(stepPart);
    }

    let start = spec.min;
    let end = spec.max;

    if (rangePart !== "*") {
      const bounds = rangePart.split("-");

      if (bounds.length > 2) {
        return null;
      }

      const first = endpoint(spec, bounds[0], name);

      if (first === null) {
        return null;
      }

      start = first;

      if (bounds.length === 2) {
        const second = endpoint(spec, bounds[1], name);

        if (second === null || second < first) {
          return null;
        }

        end = second;
      } else {
        // `a/s` means "from a to the end of the range, every s"; a bare `a` is just a.
        end = stepPart === "" ? first : spec.max;
      }
    }

    for (let value = start; value <= end; value += step) {
      values.add(value);
    }
  }

  return values.size > 0 ? values : null;
}

/** null when the field is valid, otherwise a message naming what is wrong. */
export function validateCronField(field: string, name: CronFieldName): string | null {
  const spec = FIELD_SPECS[name];

  if (expandCronField(field, name)) {
    return null;
  }

  const range = `${String(spec.min)}-${String(spec.max)}`;
  const names = spec.names ? ` or ${spec.names.slice(0, 3).join("/")}…` : "";
  return `${spec.label} must be ${range}${names}, a range, a list, or a */step`;
}

/** null when the whole five-field expression is valid, otherwise the first problem found. */
export function validateCron(expression: string): string | null {
  const trimmed = expression.trim();

  if (!trimmed) {
    return "A cron expression is required";
  }

  const fields = splitCron(trimmed);

  if (!fields) {
    const count = trimmed.split(/\s+/).length;
    return `Expected 5 fields (minute hour day month weekday), found ${String(count)}`;
  }

  for (const name of CRON_FIELD_ORDER) {
    const problem = validateCronField(fields[name], name);

    if (problem) {
      return problem;
    }
  }

  return null;
}

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}

function listLabel(values: number[], labels: string[], offset: number): string {
  const named = values.map((value) => labels[value - offset]);

  if (named.length === 1) {
    return named[0];
  }

  // a contiguous run reads better as a range than as a list.
  const contiguous = values.every((value, index) => index === 0 || value === values[index - 1] + 1);

  if (contiguous && named.length > 2) {
    return `${named[0]} through ${named[named.length - 1]}`;
  }

  return `${named.slice(0, -1).join(", ")} and ${named[named.length - 1]}`;
}

function everyStep(field: string): number | null {
  const match = /^\*\/(\d+)$/.exec(field.trim());
  return match ? Number(match[1]) : null;
}

/** a one-line english reading of a five-field expression, or "" when it cannot be parsed. */
export function describeCron(expression: string): string {
  const fields = splitCron(expression);

  if (!fields || validateCron(expression)) {
    return "";
  }

  const minutes = [...(expandCronField(fields.minute, "minute") ?? [])].sort((a, b) => a - b);
  const hours = [...(expandCronField(fields.hour, "hour") ?? [])].sort((a, b) => a - b);
  const parts: string[] = [];
  const minuteStep = everyStep(fields.minute);
  const hourStep = everyStep(fields.hour);

  if (fields.minute === "*" && fields.hour === "*") {
    parts.push("Every minute");
  } else if (minuteStep !== null && fields.hour === "*") {
    parts.push(`Every ${String(minuteStep)} minutes`);
  } else if (fields.minute === "*") {
    parts.push("Every minute");
  } else if (hourStep !== null && minutes.length === 1) {
    parts.push(`Every ${String(hourStep)} hours at :${pad(minutes[0])}`);
  } else if (fields.hour === "*" && minutes.length === 1) {
    parts.push(`Hourly at :${pad(minutes[0])}`);
  } else if (minutes.length === 1 && hours.length === 1) {
    parts.push(`At ${pad(hours[0])}:${pad(minutes[0])}`);
  } else if (minutes.length === 1) {
    parts.push(
      `At ${hours.map((hour) => `${pad(hour)}:${pad(minutes[0])}`).join(", ")}`,
    );
  } else {
    parts.push(
      `At minute ${minutes.join(", ")} of ${fields.hour === "*" ? "every hour" : `hour ${hours.join(", ")}`}`,
    );
  }

  if (fields.hour !== "*" && (fields.minute === "*" || minuteStep !== null)) {
    parts.push(`during hour ${hours.join(", ")}`);
  }

  if (fields.dayOfMonth !== "*") {
    const days = [...(expandCronField(fields.dayOfMonth, "dayOfMonth") ?? [])].sort(
      (a, b) => a - b,
    );
    parts.push(`on day ${days.join(", ")} of the month`);
  }

  if (fields.dayOfWeek !== "*") {
    const days = [...(expandCronField(fields.dayOfWeek, "dayOfWeek") ?? [])].sort((a, b) => a - b);
    parts.push(`on ${listLabel(days, DAY_LABELS, 0)}`);
  }

  if (fields.month !== "*") {
    const months = [...(expandCronField(fields.month, "month") ?? [])].sort((a, b) => a - b);
    parts.push(`in ${listLabel(months, MONTH_LABELS, 1)}`);
  }

  return `${parts.join(", ")} (UTC)`;
}

/**
 * the next `count` occurrences at or after `from`, in UTC.
 *
 * walks the calendar rather than every minute: a schedule like `0 0 1 1 *` is one match in half a
 * million minutes, and a minute-by-minute scan of it would hang the editor on every keystroke.
 */
export function nextCronRuns(expression: string, count = 3, from: Date = new Date()): Date[] {
  const fields = splitCron(expression);

  if (!fields || validateCron(expression)) {
    return [];
  }

  const minutes = expandCronField(fields.minute, "minute");
  const hours = expandCronField(fields.hour, "hour");
  const daysOfMonth = expandCronField(fields.dayOfMonth, "dayOfMonth");
  const months = expandCronField(fields.month, "month");
  const daysOfWeek = expandCronField(fields.dayOfWeek, "dayOfWeek");

  if (!minutes || !hours || !daysOfMonth || !months || !daysOfWeek) {
    return [];
  }

  // vixie semantics: with both day fields restricted a date matches if *either* does.
  const domRestricted = fields.dayOfMonth.trim() !== "*";
  const dowRestricted = fields.dayOfWeek.trim() !== "*";

  const dayMatches = (date: Date): boolean => {
    const dom = daysOfMonth.has(date.getUTCDate());
    const dow = daysOfWeek.has(date.getUTCDay());

    if (domRestricted && dowRestricted) {
      return dom || dow;
    }

    return domRestricted ? dom : dowRestricted ? dow : true;
  };

  const results: Date[] = [];
  const cursor = new Date(from.getTime());
  cursor.setUTCSeconds(0, 0);
  cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
  // five years of calendar steps is far past any schedule worth previewing, and bounds the walk for
  // a combination that never matches at all (february 30th).
  const limit = new Date(cursor.getTime());
  limit.setUTCFullYear(limit.getUTCFullYear() + 5);

  while (results.length < count && cursor < limit) {
    if (!months.has(cursor.getUTCMonth() + 1)) {
      cursor.setUTCMonth(cursor.getUTCMonth() + 1, 1);
      cursor.setUTCHours(0, 0, 0, 0);
      continue;
    }

    if (!dayMatches(cursor)) {
      cursor.setUTCDate(cursor.getUTCDate() + 1);
      cursor.setUTCHours(0, 0, 0, 0);
      continue;
    }

    if (!hours.has(cursor.getUTCHours())) {
      cursor.setUTCHours(cursor.getUTCHours() + 1, 0, 0, 0);
      continue;
    }

    if (!minutes.has(cursor.getUTCMinutes())) {
      cursor.setUTCMinutes(cursor.getUTCMinutes() + 1, 0, 0);
      continue;
    }

    results.push(new Date(cursor.getTime()));
    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1, 0, 0);
  }

  return results;
}

/** `2026-08-18 09:00 UTC`, the form the preview list shows. */
export function formatCronRun(date: Date): string {
  const day = `${String(date.getUTCFullYear())}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`;
  return `${day} ${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())} UTC`;
}

/** the selectable values for a field, for the builder's multi-selects. */
export function cronFieldOptions(name: CronFieldName): { value: number; label: string }[] {
  const spec = FIELD_SPECS[name];
  const options: { value: number; label: string }[] = [];

  for (let value = spec.min; value <= spec.max; value += 1) {
    const label =
      name === "dayOfWeek"
        ? DAY_LABELS[value]
        : name === "month"
          ? MONTH_LABELS[value - 1]
          : String(value);
    options.push({ value, label });
  }

  return options;
}

export function cronFieldLabel(name: CronFieldName): string {
  return FIELD_SPECS[name].label;
}
