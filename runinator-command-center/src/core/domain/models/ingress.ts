import type { JsonValue } from "../json";

export interface IngressTarget {
  kind: "workflow" | "pipeline";
  id: string;
}

export interface IngressAdmission {
  id: string;
  org_id: string | null;
  scope: string;
  correlation_key: string;
  generation: number;
  target: IngressTarget;
  status: "active" | "terminal";
  workflow_run_id: string | null;
  pipeline_run_id: string | null;
  policy: JsonValue;
  created_at: string;
  updated_at: string;
}

export interface IngressInboxEntry {
  id: string;
  admission_id: string;
  sequence: number;
  generation: number;
  source: string;
  event_id: string;
  event_type: string;
  correlation_key: string;
  payload: JsonValue;
  occurred_at: string | null;
  received_at: string;
  disposition: "started" | "recorded" | "queued" | "interrupt_requested" | "requeued" | "rejected";
  queue_state: "none" | "queued" | "claimed" | "promoted";
  queue_position: number | null;
  promoted_generation: number | null;
  workflow_run_id: string | null;
  pipeline_run_id: string | null;
}
