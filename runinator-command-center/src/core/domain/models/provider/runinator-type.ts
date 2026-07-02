import type { JsonValue } from "../../json";

interface RuninatorTypeRange {
  type: "range";
  base: RuninatorType;
  min?: number;
  max?: number;
}

interface RuninatorTypeArray {
  type: "array";
  items: RuninatorType;
}

interface RuninatorTypeMap {
  type: "map";
  values: RuninatorType;
}

interface RuninatorTypeStruct {
  type: "struct";
  fields: Record<string, RuninatorField>;
  additional?: RuninatorType;
}

interface RuninatorTypeUnion {
  type: "union";
  variants: RuninatorType[];
}

export type RuninatorType =
  | { type: "null" }
  | { type: "boolean" }
  | { type: "integer" }
  | { type: "number" }
  | { type: "duration" }
  | { type: "string" }
  | { type: "enum"; values: JsonValue[] }
  | RuninatorTypeRange
  | RuninatorTypeArray
  | RuninatorTypeMap
  | RuninatorTypeStruct
  | RuninatorTypeUnion
  | { type: "any" };

export interface RuninatorField {
  ty: RuninatorType;
  required: boolean;
}

/** narrow workflow input_type from the wire definition blob. */
export function asRuninatorType(value: JsonValue | undefined): RuninatorType | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  const type = (value as { type?: unknown }).type;
  return typeof type === "string" ? (value as RuninatorType) : null;
}
