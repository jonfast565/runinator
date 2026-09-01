import type { ScheduleSpec, ScheduleWeekday } from "../domain/models";
import { describeCron, nextCronRuns, validateCron } from "./cron";

export const SCHEDULE_WEEKDAYS: { value: ScheduleWeekday; short: string; label: string }[] = [
  { value: "monday", short: "Mon", label: "Monday" },
  { value: "tuesday", short: "Tue", label: "Tuesday" },
  { value: "wednesday", short: "Wed", label: "Wednesday" },
  { value: "thursday", short: "Thu", label: "Thursday" },
  { value: "friday", short: "Fri", label: "Friday" },
  { value: "saturday", short: "Sat", label: "Saturday" },
  { value: "sunday", short: "Sun", label: "Sunday" },
];

export const RRULE_TEMPLATES = [
  { label: "Daily", rule: "FREQ=DAILY" },
  { label: "Weekdays", rule: "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR" },
  { label: "Weekly", rule: "FREQ=WEEKLY" },
  { label: "Monthly", rule: "FREQ=MONTHLY" },
  { label: "First weekday monthly", rule: "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1" },
];

export function browserTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

export function defaultSchedule(window = false): ScheduleSpec {
  return {
    recurrence: {
      kind: "weekdays",
      days: ["monday", "tuesday", "wednesday", "thursday", "friday"],
      hour: window ? 3 : 9,
      minute: 0,
      second: 0,
    },
    timezone: browserTimezone(),
    duration_seconds: window ? 7_200 : 0,
  };
}

export function validateSchedule(schedule: ScheduleSpec, window = false): string {
  if (!schedule.timezone.trim()) {return "Choose an IANA timezone.";}
  if (window && schedule.duration_seconds <= 0) {return "A window needs a positive duration.";}
  if (!window && schedule.duration_seconds !== 0) {return "A firing schedule cannot have a duration.";}
  const recurrence = schedule.recurrence;

  if (recurrence.kind === "once") {
    return Number.isNaN(Date.parse(recurrence.at)) ? "Choose a valid occurrence date." : "";
  }

  if (recurrence.kind === "cron") {return validateCron(recurrence.expression) ?? "";}

  if (recurrence.kind === "weekdays") {
    if (!recurrence.days.length) {return "Select at least one weekday.";}

    if (recurrence.hour < 0 || recurrence.hour > 23 || recurrence.minute < 0 || recurrence.minute > 59) {
      return "Choose a valid local time.";
    }

    return "";
  }

  if (Number.isNaN(Date.parse(recurrence.dtstart))) {return "Choose a valid DTSTART.";}
  const rule = recurrence.rule.trim().replace(/^RRULE:/i, "");

  if (!/^FREQ=(SECONDLY|MINUTELY|HOURLY|DAILY|WEEKLY|MONTHLY|YEARLY)(;|$)/i.test(rule)) {
    return "RRULE must begin with a supported FREQ.";
  }

  return "";
}

export function describeSchedule(schedule: ScheduleSpec): string {
  const zone = schedule.timezone || "UTC";
  const recurrence = schedule.recurrence;
  let text: string;

  if (recurrence.kind === "once") {
    text = `Once on ${formatOccurrence(new Date(recurrence.at), zone)}`;
  } else if (recurrence.kind === "cron") {
    text = `${describeCron(recurrence.expression).replace(" (UTC)", "")} · ${zone}`;
  } else if (recurrence.kind === "weekdays") {
    const days = recurrence.days
      .map((day) => SCHEDULE_WEEKDAYS.find((entry) => entry.value === day)?.short ?? day)
      .join(", ");
    text = `${days} at ${pad(recurrence.hour)}:${pad(recurrence.minute)} · ${zone}`;
  } else {
    text = `${recurrence.rule.replace(/^RRULE:/i, "")} · ${zone}`;
  }

  return schedule.duration_seconds > 0
    ? `${text} · ${formatDuration(schedule.duration_seconds)} window`
    : text;
}

export function previewSchedule(schedule: ScheduleSpec, count = 4, from = new Date()): Date[] {
  if (validateSchedule(schedule, schedule.duration_seconds > 0)) {return [];}
  const recurrence = schedule.recurrence;

  if (recurrence.kind === "once") {
    const date = new Date(recurrence.at);
    return date > from ? [date] : [];
  }

  if (recurrence.kind === "cron" && schedule.timezone === "UTC") {
    return nextCronRuns(recurrence.expression, count, from);
  }

  if (recurrence.kind === "weekdays") {
    return previewWeekdays(schedule, count, from);
  }

  if (recurrence.kind === "rrule") {
    const byDay = /(?:^|;)BYDAY=([^;]+)/i.exec(recurrence.rule)?.[1];
    const days = byDay?.split(",").map(rruleWeekday).filter(Boolean) as ScheduleWeekday[] | undefined;
    const start = new Date(recurrence.dtstart);

    if (/FREQ=WEEKLY/i.test(recurrence.rule) && days?.length) {
      const parts = zonedParts(start, schedule.timezone);
      return previewWeekdays(
        {
          ...schedule,
          recurrence: { kind: "weekdays", days, hour: parts.hour, minute: parts.minute, second: parts.second },
        },
        count,
        new Date(Math.max(from.getTime(), start.getTime() - 1)),
      );
    }

    if (/FREQ=DAILY/i.test(recurrence.rule)) {
      const results: Date[] = [];
      const interval = Number(/(?:^|;)INTERVAL=(\d+)/i.exec(recurrence.rule)?.[1] ?? 1);

      for (let cursor = new Date(start); results.length < count; cursor = new Date(cursor.getTime() + interval * 86_400_000)) {
        if (cursor > from) {results.push(new Date(cursor));}
      }

      return results;
    }

    return start > from ? [start] : [];
  }

  return [];
}

export function formatOccurrence(date: Date, timezone: string): string {
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: timezone || "UTC",
    timeZoneName: "short",
  }).format(date);
}

function previewWeekdays(schedule: ScheduleSpec, count: number, from: Date): Date[] {
  if (schedule.recurrence.kind !== "weekdays") {return [];}
  const wanted = new Set(schedule.recurrence.days);
  const results: Date[] = [];
  const cursor = new Date(from.getTime());
  cursor.setUTCSeconds(0, 0);
  cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
  const limit = cursor.getTime() + 21 * 86_400_000;

  while (results.length < count && cursor.getTime() < limit) {
    const parts = zonedParts(cursor, schedule.timezone);

    if (
      wanted.has(SCHEDULE_WEEKDAYS[(parts.weekday + 6) % 7].value) &&
      parts.hour === schedule.recurrence.hour &&
      parts.minute === schedule.recurrence.minute
    ) {
      results.push(new Date(cursor));
    }

    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
  }

  return results;
}

function zonedParts(date: Date, timezone: string) {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: timezone,
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  }).formatToParts(date);
  const value = (kind: "hour" | "minute" | "second") =>
    Number(parts.find((part) => part.type === kind)?.value ?? 0);
  const weekdayName = parts.find((part) => part.type === "weekday")?.value ?? "Sun";
  return { weekday: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].indexOf(weekdayName), hour: value("hour"), minute: value("minute"), second: value("second") };
}

function rruleWeekday(value: string): ScheduleWeekday | undefined {
  return ({ MO: "monday", TU: "tuesday", WE: "wednesday", TH: "thursday", FR: "friday", SA: "saturday", SU: "sunday" } as Record<string, ScheduleWeekday>)[value.replace(/^[+-]?\d+/, "").toUpperCase()];
}

function pad(value: number): string { return String(value).padStart(2, "0"); }

function formatDuration(seconds: number): string {
  if (seconds % 3_600 === 0) {return `${String(seconds / 3_600)}h`;}
  if (seconds % 60 === 0) {return `${String(seconds / 60)}m`;}
  return `${String(seconds)}s`;
}
