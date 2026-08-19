import type { RunSummary } from "../run/run-summary";
import type { PipelineRun } from "./pipeline-run";

// a pipeline run with the member workflow runs it started. mirrors the workflow run detail shape so
// the ui can render the same list+detail layout and click through from a member step to its run.
export interface PipelineRunDetail {
  run: PipelineRun;
  members: RunSummary[];
  attempts: PipelineMemberAttempt[];
  edges: PipelineRunEdgeState[];
  joins: PipelineRunJoinState[];
}

export interface PipelineMemberAttempt {
  id: string;
  pipeline_run_id: string;
  member_key: string;
  workflow_id: string;
  attempt: number;
  workflow_run_id: string | null;
  status: string;
  parameters: unknown;
  result: unknown;
  message: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface PipelineRunEdgeState { link_id: string; state: string }
export interface PipelineRunJoinState {
  target: string;
  mode: "all" | "any" | "first_success";
  state: string;
  satisfied_inputs: number;
  total_inputs: number;
}
