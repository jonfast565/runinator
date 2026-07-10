import type { JsonValue } from "../../json";
import type { ActionParameterMetadata } from "../provider/action-metadata";
import type { WorkflowNodeKind } from "../workflow/node-kind";

// which region of a workflow node's json a field reads from and writes to. mirrors the backend
// `LocationBase`: node kinds do not all store inputs under `parameters` (wait -> node.wait, loop ->
// node.max_iterations, action -> node.action, condition -> node.transitions, ...).
export type NodeFieldLocationBase =
  | "parameters"
  | "wait"
  | "condition"
  | "action"
  | "transitions"
  | "top_level";

export interface FieldLocation {
  base: NodeFieldLocationBase;
  path: string[];
}

// a form field: the shared parameter schema plus a location and an optional widget hint.
export interface NodeFieldMetadata extends ActionParameterMetadata {
  location: FieldLocation;
  widget?: string | null;
}

export type EdgeTaxonomy = "direct" | "branch" | "control";

export interface NodeEdgeSlot {
  key: string;
  label: string;
  description?: string | null;
  taxonomy: EdgeTaxonomy;
  target: FieldLocation;
  multiple: boolean;
  editable_label: boolean;
  editable_condition: boolean;
  orderable: boolean;
}

export interface WorkflowNodeKindMetadata {
  kind: WorkflowNodeKind;
  label: string;
  icon: string;
  description: string;
  category: string;
  protected: boolean;
  terminal: boolean;
  addable: boolean;
  supports_predicate_edges: boolean;
  fields: NodeFieldMetadata[];
  edge_slots: NodeEdgeSlot[];
  default_template: JsonValue;
}
