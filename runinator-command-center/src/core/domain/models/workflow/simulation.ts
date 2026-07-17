import type { JsonValue } from "../../json";
import type { WorkflowDefinition } from "./definition";
import type { WorkflowNodeKind } from "./node-kind";

// request body for a server-side dry-run (branch preview). Mirrors runinator-models
// `WorkflowSimulateRequest`: the workflow is walked with the reducer's evaluators against live
// config, publishing no actions; `replay_run` replays that run's recorded outputs.
export interface WorkflowSimulateRequest {
  workflow: WorkflowDefinition;
  inputs?: JsonValue;
  replay_run?: string | null;
}

// one visited node in a dry-run walk. Mirrors runinator-workflows `SimStep`.
export interface SimStep {
  node_id: string;
  kind: WorkflowNodeKind;
  status: string;
  // the next node the walk routed to, when the node had an outgoing edge.
  next?: string;
  // the value recorded as this node's output, when it produced one.
  output?: JsonValue;
  // a short reason string mirroring the reducer's transition reasons.
  note?: string;
}

// the result of a dry-run walk. Mirrors runinator-workflows `SimulationRun`.
export interface SimulationRun {
  status: string;
  steps: SimStep[];
  output: JsonValue;
  // set when the walk could not continue (unsupported kind, missing node, blocked with no edge).
  error?: string;
}
