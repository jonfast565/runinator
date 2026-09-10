import type { WorkflowNodeRun } from "../domain/models";

/** Whether a projected timeline row describes runtime infrastructure rather than an authored step. */
export function isSystemTimelineEvent(node: WorkflowNodeRun): boolean {
  return typeof node.state?.workspace_phase === "string";
}
