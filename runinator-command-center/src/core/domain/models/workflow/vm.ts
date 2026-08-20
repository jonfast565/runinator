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
  revision: number;
}

export interface WorkflowEffect {
  version: number;
  id: string;
  workflow_run_id: string;
  continuation_id: string;
  sequence: number;
  attempt: number;
  request: JsonValue;
  status: string;
  result?: JsonValue | null;
  message?: string | null;
  created_at: number;
  updated_at: number;
  finished_at?: number | null;
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
