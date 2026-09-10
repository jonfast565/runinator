import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleWorkflowsApi = Pick<
  typeof Api,
  | "createWorkflowRun"
  | "duplicateWorkflow"
  | "exportWorkflowBundle"
  | "fetchWorkflowRevision"
  | "fetchWorkflowRevisions"
  | "fetchWorkflows"
  | "restoreWorkflowRevision"
  | "fetchPipelines"
>;
