import type { JsonValue } from "../../json";

export type OrchestrationStatus =
  | "pending"
  | "running"
  | "waiting"
  | "suspended"
  | "completed"
  | "failed"
  | "terminated";

export interface OrchestrationBinding {
  id: string;
  admission_id: string;
  org_id?: string | null;
  scope: string;
  correlation_key: string;
  generation: number;
  pipeline_id: string;
  pipeline_revision: number;
  pipeline_digest: string;
  policy: OrchestrationPolicy;
  status: OrchestrationStatus;
  current_phase?: string | null;
  current_attempt: number;
  current_epoch: number;
  restart_member?: string | null;
  resume_existing_epoch: boolean;
  subject_revision?: string | null;
  resources: JsonValue;
  budgets: Record<string, number>;
  last_reduced_sequence: number;
  version: number;
  reducer_lease_owner?: string | null;
  reducer_leased_until?: string | null;
  created_at: string;
  updated_at: string;
  finished_at?: string | null;
}

export interface OrchestrationPolicy {
  intents: Record<string, IntentPolicy>;
  phases: Record<string, unknown>;
  budgets: Record<string, { attempts: number; exhausted: "fail" | "pause" | "terminate" }>;
  defaults: JsonValue;
}

export interface IntentPolicy {
  effect: "terminate" | "suspend" | "resume" | "supersede" | "observe" | "signal";
  priority: number;
  coalesce_seconds?: number | null;
  stop?: "pause" | "cancel" | "none";
  restart?: { kind: "entry" | "current" | "member"; member?: string };
}

export interface OrchestrationEpoch {
  id: string;
  binding_id: string;
  epoch: number;
  pipeline_run_id?: string | null;
  start_member?: string | null;
  parameters: JsonValue;
  status: string;
  reason: string;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
}

export interface OrchestrationReduction {
  id: string;
  binding_id: string;
  inbox_event_id: string;
  sequence: number;
  matched_intents: string[];
  winner?: string | null;
  suppressed_intents: string[];
  binding_version: number;
  disposition: string;
  detail: JsonValue;
  created_at: string;
}

export interface OrchestrationEvidence {
  id: string;
  binding_id: string;
  epoch?: number | null;
  kind: string;
  subject_revision?: string | null;
  payload: JsonValue;
  source_event_id?: string | null;
  created_at: string;
}

export interface OrchestrationCommand {
  id: string;
  binding_id: string;
  epoch: number;
  command_type: string;
  operation_key: string;
  payload: JsonValue;
  status: string;
  attempts: number;
  result: JsonValue;
  created_at: string;
  updated_at: string;
}
