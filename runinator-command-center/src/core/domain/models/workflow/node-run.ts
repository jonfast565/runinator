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

/**
 * Timeline rows can be projections of journal entries as well as durable effects. Only the latter
 * own streamed output, and their effect id is kept in the projection state rather than the row id.
 */
export function workflowEffectId(node: Pick<WorkflowNodeRun, "state">): string | null {
  const effectId = node.state?.effect_id;
  return typeof effectId === "string" && effectId.length > 0 ? effectId : null;
}
