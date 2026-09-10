import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type PipelineDataApi = Pick<typeof Api, "fetchWorkflowTriggers" | "fetchWorkflows">;
