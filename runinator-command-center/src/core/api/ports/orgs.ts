import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type OrgsApi = Pick<
  typeof Api,
  "createOrg" | "listMyOrgs" | "switchOrg" | "switchPlatform" | "updateOrg"
>;
