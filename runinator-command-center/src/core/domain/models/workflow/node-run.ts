import type { JsonRecord, JsonValue } from "../../json";

export interface WorkflowNodeRun {
  id: string;
  workflow_run_id: string;
  node_id: string;
  status: string;
  attempt: number;
  parameters: JsonRecord;
  output_json?: JsonValue;
  state?: JsonRecord;
  transition_reason?: string | null;
  prev_node_run_id?: string | null;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
  message: string | null;
}
