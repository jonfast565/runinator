import type { JsonRecord, JsonValue } from "../types/json";
import { asJsonRecord, isJsonObject } from "../types/json";

export function parseObject(text: string, fallback: JsonRecord): JsonRecord {
  try {
    const value: JsonValue = JSON.parse(text || "{}") as JsonValue;
    return isJsonObject(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function parseRequiredObject(text: string): JsonRecord | null {
  try {
    const value: JsonValue = JSON.parse(text || "{}") as JsonValue;

    if (isJsonObject(value)) {
      return value;
    }
  } catch {
    // surfaced by caller.
  }

  return null;
}

export function parseRequiredJson(text: string): JsonValue | null {
  try {
    return JSON.parse(text || "null") as JsonValue;
  } catch {
    // surfaced by caller.
  }

  return null;
}

export function cloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

// legacy alias used by a few call sites.
export function parseObjectRecord(text: string, fallback: JsonRecord): JsonRecord {
  return parseObject(text, fallback);
}

export { asJsonRecord as asJsonObject };
