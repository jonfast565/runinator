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
  /** the thread of control that produced this run, so a run with fan-out (or an interrupt handler)
   * can attribute each step to a branch instead of leaving it unexplained. */
  cursor_id?: string | null;
  /** true when a debugger "what if" cursor produced this. */
  speculative?: boolean;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
  message: string | null;
}
