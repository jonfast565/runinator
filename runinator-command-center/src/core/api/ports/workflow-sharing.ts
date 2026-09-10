import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowSharingApi = Pick<
  typeof Api,
  "createResourceGrant" | "listResourceGrants" | "revokeResourceGrant" | "setWorkflowOwner"
>;
