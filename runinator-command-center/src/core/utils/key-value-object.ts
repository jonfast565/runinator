import type { JsonRecord } from "../domain/models";
import { asJsonValue } from "../domain/json";

export interface RenameObjectKeyResult {
  value: JsonRecord;
  error: string;
}

export function uniqueObjectKey(record: JsonRecord, base = "key"): string {
  let index = 1;
  let key = base;

  while (Object.prototype.hasOwnProperty.call(record, key)) {
    index += 1;
    key = `${base}_${String(index)}`;
  }

  return key;
}

export function renameObjectKey(
  record: JsonRecord,
  previousKey: string,
  nextKey: string,
): RenameObjectKeyResult {
  const trimmed = nextKey.trim();

  if (!trimmed) {
    return { value: record, error: "Key is required" };
  }

  if (trimmed !== previousKey && Object.prototype.hasOwnProperty.call(record, trimmed)) {
    return { value: record, error: "Key already exists" };
  }

  if (trimmed === previousKey) {
    return { value: record, error: "" };
  }

  const next: JsonRecord = {};

  for (const [key, value] of Object.entries(record)) {
    next[key === previousKey ? trimmed : key] = value;
  }

  return { value: next, error: "" };
}

export function setObjectValue(record: JsonRecord, key: string, value: unknown): JsonRecord {
  if (!key.trim()) {
    return record;
  }

  return { ...record, [key]: asJsonValue(value) };
}

export function removeObjectKey(record: JsonRecord, key: string): JsonRecord {
  return Object.fromEntries(Object.entries(record).filter(([entryKey]) => entryKey !== key));
}
