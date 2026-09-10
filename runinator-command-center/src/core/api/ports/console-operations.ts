import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleOperationsApi = Pick<
  typeof Api,
  | "approveApproval"
  | "fetchApprovals"
  | "fetchWorkflowEffectOutput"
  | "fetchProviders"
  | "fetchSupervisorStatus"
  | "fetchWorkflowRuns"
  | "fetchWorkflows"
  | "rejectApproval"
>;
