import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type SchedulesApi = Pick<
  typeof Api,
  "createFreezeWindow" | "deleteFreezeWindow" | "fetchFreezeWindows" | "updateFreezeWindow"
>;
