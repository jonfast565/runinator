// scheduling policy: portable recurrence plus what suspends/replays scheduled work.
// mirrors runinator-models/src/schedules.rs.

export type ScheduleWeekday =
  | "monday"
  | "tuesday"
  | "wednesday"
  | "thursday"
  | "friday"
  | "saturday"
  | "sunday";

export type ScheduleRecurrence =
  | { kind: "once"; at: string }
  | { kind: "cron"; expression: string }
  | {
      kind: "weekdays";
      days: ScheduleWeekday[];
      hour: number;
      minute: number;
      second: number;
    }
  | { kind: "rrule"; rule: string; dtstart: string };

export interface ScheduleSpec {
  recurrence: ScheduleRecurrence;
  timezone: string;
  duration_seconds: number;
}

// a scheduled suspension of trigger firing. a window with no workflow_id freezes every workflow in
// its org; one with no org_id freezes the whole platform.
export interface FreezeWindow {
  id: string;
  org_id?: string | null;
  workflow_id?: string | null;
  name: string;
  reason?: string | null;
  starts_at: string;
  ends_at: string;
  schedule?: ScheduleSpec | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export type NewFreezeWindow = Omit<
  FreezeWindow,
  "id" | "created_at" | "updated_at"
>;

export interface BackfillRequest {
  from: string;
  to: string;
  limit?: number | null;
  // report the slots that would fire without creating any runs.
  dry_run: boolean;
}

export interface BackfillResponse {
  trigger_id: string;
  workflow_id: string;
  // slots inside the range that already had a firing recorded, so they were left alone.
  already_fired: number;
  fired: number;
  truncated: boolean;
  dry_run: boolean;
  run_ids: string[];
  slots: string[];
}

export type CalendarScope = "user" | "organization" | "platform";

export interface CalendarSubscription {
  id: string;
  principal_id: string;
  scope: { kind: "platform" | "organization" | "team" | "user"; id?: string | null };
  created_at: string;
}

export interface CalendarSubscriptionSecret {
  subscription: CalendarSubscription;
  token: string;
}
