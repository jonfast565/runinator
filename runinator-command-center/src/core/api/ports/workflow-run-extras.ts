import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowRunExtrasApi = Pick<
  typeof Api,
  | "deliverSignal"
  | "fetchWorkflowEffectOutput"
  | "fetchWorkflowRunArtifacts"
  | "settleWorkflowEffect"
>;
