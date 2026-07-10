import type { JsonValue } from "../../json";
import type { ActionParameterMetadata } from "../provider/action-metadata";
import type { WorkflowTriggerKind } from "../workflow/trigger";

// a trigger config field. trigger config lives in the untyped `configuration` blob, so there is no
// `FieldLocation` — every field is a key within `configuration`.
export interface UiField extends ActionParameterMetadata {
  widget?: string | null;
}

export interface WorkflowTriggerKindMetadata {
  kind: WorkflowTriggerKind;
  label: string;
  icon: string;
  description: string;
  fields: UiField[];
  default_configuration: JsonValue;
}
