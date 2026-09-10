import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type RexrapLanguageApi = Pick<
  typeof Api,
  "analyzeRexRap" | "completeRexRap" | "formatRexRap" | "hoverRexRap"
>;
