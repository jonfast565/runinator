import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleSessionApi = Pick<
  typeof Api,
  | "createPipelineRun"
  | "createWorkflowRun"
  | "retryPipelineMember"
  | "fetchWorkflows"
  | "fetchPipelines"
>;
