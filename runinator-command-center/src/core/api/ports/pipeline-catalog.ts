import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type PipelineCatalogApi = Pick<
  typeof Api,
  | "deletePipeline"
  | "fetchPipelineRexRap"
  | "fetchPipelines"
  | "savePipeline"
  | "savePipelineRexRap"
  | "setPipelineOwner"
>;
