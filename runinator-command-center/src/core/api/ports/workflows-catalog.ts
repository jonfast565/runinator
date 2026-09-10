import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowsCatalogApi = Pick<
  typeof Api,
  | "decompileToRexRap"
  | "deleteWorkflow"
  | "deleteWorkflowTrigger"
  | "duplicateWorkflow"
  | "fetchWorkflowTriggers"
  | "fetchWorkflows"
  | "importPackArchive"
  | "saveWorkflow"
  | "saveWorkflowRexRap"
  | "saveWorkflowTrigger"
>;
