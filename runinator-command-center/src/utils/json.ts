import type { JsonRecord } from "../types/models";

export function parseObject(text: string, fallback: JsonRecord): JsonRecord {
  try {
    const value: unknown = JSON.parse(text || "{}");
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as JsonRecord)
      : fallback;
  } catch {
    return fallback;
  }
}

export function parseRequiredObject(text: string): JsonRecord | null {
  try {
    const value: unknown = JSON.parse(text || "{}");

    if (value && typeof value === "object" && !Array.isArray(value)) {
      return value as JsonRecord;
    }
  } catch {
    // surfaced by caller.
  }

  return null;
}

export function parseRequiredJson(text: string): unknown {
  try {
    return JSON.parse(text || "null");
  } catch {
    // surfaced by caller.
  }

  return null;
}

export function cloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
