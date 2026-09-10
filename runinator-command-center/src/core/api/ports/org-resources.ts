import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type OrgResourcesApi = Pick<
  typeof Api,
  "fetchOrgNodes" | "fetchOrgQuota" | "fetchOrgUsage" | "fetchRateCard" | "scaleOrgNodes"
>;
