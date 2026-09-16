import type { JsonRecord, JsonValue } from "../json";

export type NotificationChannel = "in_app" | "email" | "slack";
export type NotificationSeverity = "info" | "success" | "warning" | "error";

export type NotificationInteractionState = "open" | "resolved" | "stale";
export type NotificationInteractionInput = "none" | "text" | "json";

export interface NotificationInteractionAction {
  id: string;
  label: string;
  input: NotificationInteractionInput;
}

export type NotificationInteractionTarget =
  | { kind: "effect"; workflow_run_id: string; effect_id: string; attempt: number }
  | {
      kind: "signal";
      workflow_run_id: string;
      effect_id: string;
      attempt: number;
      name: string;
    }
  | { kind: "terminal"; workflow_run_id: string; effect_id: string; attempt: number };

export interface NotificationInteraction {
  id: string;
  notification_id: string;
  org_id?: string | null;
  target: NotificationInteractionTarget;
  actions: NotificationInteractionAction[];
  state: NotificationInteractionState;
  resolved_action?: string | null;
  resolved_by?: string | null;
  resolved_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Notification {
  id: string;
  org_id?: string | null;
  source_resource_type?: string | null;
  source_resource_id?: string | null;
  workflow_run_id?: string | null;
  workflow_node_id?: string | null;
  channel: NotificationChannel;
  severity: NotificationSeverity;
  title: string;
  body?: string | null;
  target?: string | null;
  metadata?: JsonRecord;
  read_at?: string | null;
  created_at: string;
  interaction?: NotificationInteraction | null;
}

// the runtime condition a policy fires on. mirrors NotificationEvent in
// runinator-models/src/notifications.rs; the duration events need a threshold to be evaluable.
export type NotificationEvent =
  "run_failed" | "node_retry_exhausted" | "run_sla_breached" | "run_parked";

export const DURATION_NOTIFICATION_EVENTS: readonly NotificationEvent[] = [
  "run_sla_breached",
  "run_parked",
];

// the severity set a policy can carry. narrower than the severity a notification row may hold,
// which also covers the "success" value posted by the manual notify action.
export type NotificationPolicySeverity = "info" | "warning" | "critical";

export interface NotificationPolicy {
  id: string;
  org_id?: string | null;
  // null makes the policy global: it covers every workflow.
  workflow_id?: string | null;
  name: string;
  event: NotificationEvent;
  severity: NotificationPolicySeverity;
  channel: NotificationChannel;
  target?: string | null;
  provider?: string | null;
  function?: string | null;
  interactive: boolean;
  threshold_seconds?: number | null;
  enabled: boolean;
  // "rexrap" for pack-managed policies, which are reconciled on import and should not be hand-edited.
  managed_by?: string | null;
  configuration?: JsonRecord | null;
  created_at: string;
  updated_at: string;
}

export type NewNotificationPolicy = Omit<NotificationPolicy, "id" | "created_at" | "updated_at">;

export type NotificationDeliveryStatus = "pending" | "dispatched" | "delivered" | "failed";

export interface NotificationDelivery {
  id: string;
  notification_id: string;
  policy_id?: string | null;
  channel: NotificationChannel;
  target?: string | null;
  provider?: string | null;
  function?: string | null;
  workflow_run_id?: string | null;
  status: NotificationDeliveryStatus;
  attempts: number;
  last_error?: string | null;
  response?: JsonValue;
  created_at: string;
  updated_at: string;
}
