import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleLookupApi = Pick<typeof Api, "fetchPipelines" | "fetchWorkflows">;
