import type { JsonRecord, JsonValue } from "../../json";

/** Durable VM branch. Its id is the debugger and execution identity. */
export interface WorkflowContinuation {
  version: number;
  id: string;
  workflow_run_id: string;
  module_version: number;
  instruction_pointer: number;
  stack: JsonValue[];
  locals: JsonRecord;
  frames: JsonValue[];
  next_effect_sequence: number;
  parent_id?: string | null;
  fork_key?: string | null;
  awaiting_effect_id?: string | null;
  status: string;
  operator_paused?: boolean;
  revision: number;
}

export interface WorkflowEffect {
  version: number;
  id: string;
  workflow_run_id: string;
  continuation_id: string;
  sequence: number;
  attempt: number;
  /** Projected from the VM journal and frozen module by the web service. */
  node_id?: string | null;
  request: JsonValue;
  status: string;
  current_executor_replica_id?: string | null;
  last_executor_replica_id?: string | null;
  result?: JsonValue | null;
  message?: string | null;
  created_at: number;
  updated_at: number;
  finished_at?: number | null;
}

export interface WorkflowEffectOutputEvent {
  event_id: string;
  effect_id: string;
  workflow_run_id: string;
  continuation_id: string;
  attempt: number;
  output:
    | { type: "chunk"; stream: string; content: string }
    | { type: "artifact"; artifact: JsonValue };
  created_at: number;
}

export interface WorkflowJournalRecord {
  version: number;
  id: string;
  workflow_run_id: string;
  sequence: number;
  continuation_id?: string | null;
  effect_id?: string | null;
  entry: JsonValue;
  created_at: number;
}

/** A graph marker projected from one continuation's instruction pointer. */
export interface WorkflowVmCursor {
  continuation_id: string;
  instruction_pointer: number;
  node_id?: string | null;
  edge_label?: string | null;
  status: string;
}
