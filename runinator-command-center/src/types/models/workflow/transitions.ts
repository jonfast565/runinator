export type WorkflowDirectTransitionKey =
  | "next"
  | "on_success"
  | "on_failure"
  | "on_timeout"
  | "on_reject";

export type WorkflowConnectionHandle = string;
