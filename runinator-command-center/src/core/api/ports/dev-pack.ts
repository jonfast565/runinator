import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type DevPackApi = Pick<
  typeof Api,
  | "applyDevPack"
  | "cancelWorkflowRun"
  | "createWorkflowRun"
  | "fetchWorkflowRun"
  | "inspectDevPack"
  | "readDevPackFile"
  | "replayWorkflowRun"
  | "writeDevPackFile"
>;
