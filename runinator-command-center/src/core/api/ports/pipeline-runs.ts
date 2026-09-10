import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type PipelineRunsApi = Pick<
  typeof Api,
  | "cancelPipelineRun"
  | "createPipelineRun"
  | "deletePipelineRun"
  | "fetchPipelineRun"
  | "fetchPipelineRuns"
  | "pausePipelineRun"
  | "resolvePipelineRun"
  | "resumePipelineRun"
  | "retryPipelineMember"
>;
