import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ResourcesApi = Pick<
  typeof Api,
  | "approveApproval"
  | "deleteAutomationEvent"
  | "fetchResourceRecords"
  | "rejectApproval"
  | "settleWorkflowEffect"
>;
