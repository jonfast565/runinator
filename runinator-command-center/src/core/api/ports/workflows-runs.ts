import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowsRunsApi = Pick<
  typeof Api,
  | "cancelWorkflowRun"
  | "deleteWorkflowRun"
  | "closeGate"
  | "continueWorkflowRun"
  | "createWorkflowRun"
  | "fetchGates"
  | "fetchWorkflowRun"
  | "fetchWorkflowRuns"
  | "openGate"
  | "pauseWorkflowRun"
  | "renameWorkflowRun"
  | "replayWorkflowRun"
  | "requestRunInterrupt"
  | "settleWorkflowEffect"
  | "resumeWorkflowRun"
  | "runWorkflowToNode"
  | "setWorkflowRunBreakpoints"
  | "setWorkflowRunPauseOnFailure"
  | "stepWorkflowRun"
>;
