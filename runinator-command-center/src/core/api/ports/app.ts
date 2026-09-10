import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AppApi = Pick<typeof Api, "fetchReplicas">;
