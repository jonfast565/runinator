import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowRevisionsApi = Pick<
  typeof Api,
  "fetchWorkflowRevision" | "fetchWorkflowRevisions" | "restoreWorkflowRevision"
>;
