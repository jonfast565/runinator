// json value algebra mirroring runinator-models::Value. use at api/persistence
// boundaries; narrow into domain structs (workflow state, gate rows, etc.) at read time.

export type JsonArray = JsonValue[];

export type JsonValue = null | boolean | number | string | JsonArray | JsonObject;

/** strict json object for typed coercion helpers. */
export interface JsonObject { [key: string]: JsonValue }

/**
 * mutable editor/wire object map. intentionally loose so graph editors, vue reactivity,
 * and api boundaries can round-trip extra keys without fighting recursive json types.
 */
export type JsonRecord = Record<string, unknown>;

export function isJsonObject(value: JsonValue | undefined): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function isJsonArray(value: JsonValue | undefined): value is JsonArray {
  return Array.isArray(value);
}

export function isJsonRecord(value: unknown): value is JsonRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

export function asJsonObject(value: JsonValue | undefined): JsonObject {
  return isJsonObject(value) ? value : {};
}

export function asJsonArray(value: JsonValue | undefined): JsonArray {
  return isJsonArray(value) ? value : [];
}

/** narrow an unknown wire/editor value to JsonValue for assignment into typed json fields. */
export function asJsonValue(value: unknown): JsonValue {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }

  if (Array.isArray(value)) {
    return value as JsonArray;
  }

  if (typeof value === "object") {
    return value as JsonObject;
  }

  if (value === undefined) {
    return "undefined";
  }

  if (typeof value === "function") {
    return value.name || "[function]";
  }

  if (typeof value === "bigint" || typeof value === "symbol") {
    return value.toString();
  }

  return null;
}

export function asJsonRecord(value: unknown): JsonRecord {
  return isJsonRecord(value) ? value : {};
}
