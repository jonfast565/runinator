import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleRexrapApi = Pick<
  typeof Api,
  | "analyzeRexRap"
  | "compileRexRap"
  | "decompileToRexRap"
  | "formatRexRap"
  | "fetchWorkflows"
  | "fetchPipelines"
>;
