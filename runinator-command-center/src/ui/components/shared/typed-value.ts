import type { JsonRecord, JsonValue, RuninatorType } from "../../../core/domain/models";

export function defaultAnyValue(kind: string): unknown {
  switch (kind) {
    case "string":
      return "";
    case "number":
      return 0;
    case "boolean":
      return false;
    case "array":
      return [];
    case "object":
      return {};
    default:
      return null;
  }
}

export function enumOptionLabel(option: JsonValue): string {
  return typeof option === "string" ? option : JSON.stringify(option);
}

export function splitLines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function uniqueRecordKey(record: JsonRecord): string {
  let index = 1;
  let key = "key";

  while (key in record) {
    index += 1;
    key = `key_${String(index)}`;
  }

  return key;
}

export function selectedUnionVariantIndex(value: unknown, variants: RuninatorType[]): number {
  const match = variants.findIndex((variant) => matchesType(value, variant));
  return match >= 0 ? match : 0;
}

export function matchesType(value: unknown, ty: RuninatorType): boolean {
  if (ty.type === "any") {
    return true;
  }

  if (ty.type === "null") {
    return value === null;
  }

  if (ty.type === "string") {
    return typeof value === "string";
  }

  if (ty.type === "file") {
    return (
      isPlainRecord(value) &&
      typeof value.id === "string" &&
      typeof value.name === "string" &&
      typeof value.path === "string" &&
      typeof value.mime_type === "string" &&
      typeof value.size_bytes === "number" &&
      typeof value.sha256 === "string"
    );
  }

  if (ty.type === "boolean") {
    return typeof value === "boolean";
  }

  if (ty.type === "integer") {
    return typeof value === "number" && Number.isInteger(value);
  }

  if (ty.type === "number") {
    return typeof value === "number" && !Number.isNaN(value);
  }

  if (ty.type === "duration") {
    return typeof value === "number" && Number.isInteger(value);
  }

  if (ty.type === "enum") {
    return ty.values.some((candidate) => JSON.stringify(candidate) === JSON.stringify(value));
  }

  if (ty.type === "range") {
    return (
      matchesType(value, ty.base) &&
      (ty.min === undefined || (typeof value === "number" && value >= ty.min)) &&
      (ty.max === undefined || (typeof value === "number" && value <= ty.max))
    );
  }

  if (ty.type === "array") {
    return Array.isArray(value);
  }

  if (ty.type === "map" || ty.type === "struct") {
    return isPlainRecord(value);
  }

  return ty.variants.some((variant) => matchesType(value, variant));
}

export function defaultValueForType(ty: RuninatorType): unknown {
  if (ty.type === "string") {
    return "";
  }

  if (ty.type === "file") {
    return null;
  }

  if (ty.type === "boolean") {
    return false;
  }

  if (ty.type === "integer" || ty.type === "number" || ty.type === "duration") {
    return 0;
  }

  if (ty.type === "enum") {
    return ty.values[0] ?? null;
  }

  if (ty.type === "range") {
    return ty.min ?? defaultValueForType(ty.base);
  }

  if (ty.type === "array") {
    return [];
  }

  if (ty.type === "map" || ty.type === "struct") {
    return {};
  }

  if (ty.type === "union") {
    return defaultValueForType(ty.variants[0] ?? { type: "any" });
  }

  return null;
}

export function defaultExpressionForType(ty: RuninatorType): JsonRecord {
  if (ty.type === "string") {
    return { $to_string: { $ref: { params: ["value"] } } };
  }

  return { $ref: { params: ["value"] } };
}

export function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) {
    return "any";
  }

  if (ty.type === "array") {
    return `${describeType(ty.items, depth + 1)}[]`;
  }

  if (ty.type === "map") {
    return `map<string, ${describeType(ty.values, depth + 1)}>`;
  }

  if (ty.type === "union") {
    return ty.variants.map((variant) => describeType(variant, depth + 1)).join(" | ");
  }

  if (ty.type === "enum") {
    return `enum[${ty.values.map((value) => JSON.stringify(value)).join(", ")}]`;
  }

  if (ty.type === "range") {
    return `${describeType(ty.base, depth + 1)} range ${String(ty.min ?? "")}..${String(ty.max ?? "")}`;
  }

  if (ty.type !== "struct") {
    return ty.type;
  }

  const entries = Object.entries(ty.fields);

  if (depth > 0 || entries.length > 3) {
    return "struct";
  }

  const fields = entries
    .map(
      ([name, field]) =>
        `${name}${field.required ? "" : "?"}: ${describeType(field.ty, depth + 1)}`,
    )
    .join("; ");
  return `{ ${fields} }`;
}

export function isPlainRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
