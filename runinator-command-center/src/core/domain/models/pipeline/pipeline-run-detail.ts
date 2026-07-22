import type { RunSummary } from "../run/run-summary";
import type { PipelineRun } from "./pipeline-run";

// a pipeline run with the member workflow runs it started. mirrors the workflow run detail shape so
// the ui can render the same list+detail layout and click through from a member step to its run.
export interface PipelineRunDetail {
  run: PipelineRun;
  members: RunSummary[];
}
