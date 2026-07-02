import type { JsonValue } from "../../json";
import type { WorkflowNodeKind } from "../workflow/node-kind";
import type { DebugMode } from "./debug-mode";

/**
 * `state.debug` wire shape. config and runtime are flattened in json (see
 * runinator-models::DebugFrame).
 */
export interface DebugFrame {
  enabled?: boolean;
  mode?: DebugMode;
  breakpoints?: string[];
  paused?: boolean;
  step_requested?: boolean;
  one_shot_breakpoint?: string | null;
  current_node_id?: string | null;
  current_node_kind?: WorkflowNodeKind | null;
  input_json?: JsonValue;
  context_json?: JsonValue;
  last_output_json?: JsonValue;
}
