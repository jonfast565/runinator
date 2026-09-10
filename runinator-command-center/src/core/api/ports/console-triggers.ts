import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleTriggersApi = Pick<
  typeof Api,
  | "backfillWorkflowTrigger"
  | "createFreezeWindow"
  | "createTriggerRun"
  | "deleteFreezeWindow"
  | "fetchDueTriggers"
  | "fetchFreezeWindows"
  | "fetchWorkflowTriggers"
  | "fetchWorkflows"
  | "fetchPipelines"
>;
