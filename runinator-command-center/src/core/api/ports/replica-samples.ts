import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ReplicaSamplesApi = Pick<typeof Api, "fetchReplicaSamples">;
