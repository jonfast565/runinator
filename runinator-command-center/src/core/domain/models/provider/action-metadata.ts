import type { JsonValue } from "../../json";
import type { DeliverySemantics } from "../orchestration/orchestration";
import type { RuninatorType } from "./runinator-type";

export interface ActionParameterMetadata {
  name: string;
  ty: RuninatorType;
  label?: string | null;
  description?: string | null;
  required: boolean;
  default_value?: JsonValue;
  secret: boolean;
}

export interface ActionResultMetadata {
  name: string;
  ty: RuninatorType;
  label?: string | null;
  description?: string | null;
}

export interface ActionMetadata {
  function_name: string;
  description?: string | null;
  parameters: ActionParameterMetadata[];
  results: ActionResultMetadata[];
  pure?: boolean;
  delivery_semantics?: DeliverySemantics;
}
