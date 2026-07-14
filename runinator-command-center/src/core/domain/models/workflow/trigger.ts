import type { JsonRecord } from "../../json";

export type WorkflowTriggerKind = "cron" | "manual" | "chained";

export interface WorkflowTrigger {
  id: string | null;
  workflow_id: string;
  kind: WorkflowTriggerKind;
  enabled: boolean;
  configuration: JsonRecord;
  next_execution: string | null;
  blackout_start: string | null;
  blackout_end: string | null;
  metadata: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}
