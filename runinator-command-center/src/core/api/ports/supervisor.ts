import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type SupervisorApi = Pick<typeof Api, "fetchSupervisorStatus">;
