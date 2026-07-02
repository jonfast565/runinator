import type { JsonRecord } from "../json";

export type NotificationChannel = "in_app" | "email" | "slack";
export type NotificationSeverity = "info" | "success" | "warning" | "error";

export interface Notification {
  id: string;
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
}
