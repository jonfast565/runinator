import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleRunsApi = Pick<
  typeof Api,
  | "cancelWorkflowRun"
  | "fetchWorkflowEffectOutput"
  | "fetchWorkflowRun"
  | "fetchWorkflowRunArtifacts"
  | "fetchWorkflowRuns"
  | "pauseWorkflowRun"
  | "renameWorkflowRun"
  | "replayWorkflowRun"
  | "fetchReplayPlan"
  | "resumeWorkflowRun"
  | "fetchWorkflows"
  | "fetchPipelines"
>;
