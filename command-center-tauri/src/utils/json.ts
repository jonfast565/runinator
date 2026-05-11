import type { JsonRecord } from "../types/models";

export function parseObject(text: string, fallback: JsonRecord): JsonRecord {
  try {
    const value = JSON.parse(text || "{}");
    return value && typeof value === "object" && !Array.isArray(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function parseRequiredObject(text: string): JsonRecord | null {
  try {
    const value = JSON.parse(text || "{}");
    if (value && typeof value === "object" && !Array.isArray(value)) return value;
  } catch {
    // surfaced by caller.
  }
  return null;
}
