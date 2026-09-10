import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type PipelineChainLinksApi = Pick<
  typeof Api,
  "deleteWorkflowTrigger" | "saveWorkflowTrigger"
>;
