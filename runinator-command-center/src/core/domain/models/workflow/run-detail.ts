import type { JsonRecord } from "../../json";
import type { RunSummary } from "../run/run-summary";
import type { WorkflowDefinition } from "./definition";
import type { WorkflowNodeRun } from "./node-run";
import type { WorkflowRunState } from "../workflow-state";

export interface WorkflowRunDetail {
  run: RunSummary & {
    workflow_id: string;
    workflow_snapshot?: JsonRecord | null;
    message?: string | null;
  };
  nodes: WorkflowNodeRun[];
  execution_state?: WorkflowRunState;
}

/** snapshot attached to a run detail, when the backend included the workflow definition. */
export function runWorkflowSnapshot(detail: WorkflowRunDetail | null | undefined): WorkflowDefinition | null {
  const snapshot = detail?.run.workflow_snapshot;
  return snapshot ? (snapshot as unknown as WorkflowDefinition) : null;
}
