import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type NodePoolsApi = Pick<typeof Api, "fetchNodeBackends" | "fetchNodes" | "scaleNodes">;
