import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ExecutionProfilesApi = Pick<
  typeof Api,
  | "deleteExecutionProfile"
  | "fetchExecutionProfileCollectionStatuses"
  | "fetchExecutionProfiles"
  | "putExecutionProfile"
  | "rotateExecutionProfile"
  | "testExecutionProfile"
>;
