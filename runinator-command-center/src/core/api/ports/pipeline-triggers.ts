import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type PipelineTriggersApi = Pick<
  typeof Api,
  "deletePipelineTrigger" | "fetchPipelineTriggers" | "savePipelineTrigger"
>;
