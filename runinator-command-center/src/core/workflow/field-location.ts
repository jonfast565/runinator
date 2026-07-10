import type { JsonRecord, JsonValue } from "../domain/json";
import { asJsonRecord, isJsonRecord } from "../domain/json";
import type { FieldLocation, NodeFieldLocationBase } from "../domain/models";
import { cloneJson } from "../utils/json";

function baseRoot(node: JsonRecord, base: NodeFieldLocationBase): unknown {
  switch (base) {
    case "parameters":
      return node.parameters;
    case "wait":
      return node.wait;
    case "condition":
      return node.condition;
    case "action":
      return node.action;
    case "transitions":
      return node.transitions;
    case "top_level":
      return node;
  }
}

function ensureBaseRoot(node: JsonRecord, base: NodeFieldLocationBase): JsonRecord {
  if (base === "top_level") {
    return node;
  }

  const key = base;
  const existing = node[key];

  if (isJsonRecord(existing)) {
    return existing;
  }

  const created: JsonRecord = {};
  node[key] = created;
  return created;
}

function getPath(root: unknown, path: string[]): unknown {
  let current: unknown = root;

  for (const segment of path) {
    if (!isJsonRecord(current)) {
      return undefined;
    }

    current = current[segment];
  }

  return current;
}

function setPath(root: JsonRecord, path: string[], value: unknown): void {
  if (path.length === 0) {
    return;
  }

  let current: JsonRecord = root;

  for (let index = 0; index < path.length - 1; index += 1) {
    const segment = path[index];
    const next = current[segment];

    if (isJsonRecord(next)) {
      current = next;
      continue;
    }

    const created: JsonRecord = {};
    current[segment] = created;
    current = created;
  }

  const leaf = path[path.length - 1];

  if (value === undefined) {
    delete current[leaf];
    return;
  }

  current[leaf] = value;
}

/** read a value from a workflow node at a catalog field location. */
export function getAtLocation(node: JsonRecord, location: FieldLocation): unknown {
  const root = baseRoot(node, location.base);

  if (location.path.length === 0) {
    return root;
  }

  return getPath(root, location.path);
}

/** immutably write a value into a workflow node at a catalog field location. */
export function setAtLocation(
  node: JsonRecord,
  location: FieldLocation,
  value: unknown,
): JsonRecord {
  const next = cloneJson(node);
  const root = ensureBaseRoot(next, location.base);

  if (location.base === "top_level") {
    if (location.path.length === 0) {
      return next;
    }

    setPath(next, location.path, value);
    return next;
  }

  if (location.path.length === 0) {
    if (value === undefined) {
      delete next[location.base];
    } else if (isJsonRecord(value)) {
      next[location.base] = value;
    } else {
      next[location.base] = asJsonRecord(value) ?? {};
    }

    return next;
  }

  setPath(root, location.path, value);
  return next;
}

/** clone a catalog default_template into a mutable node draft (without id). */
export function cloneTemplate(template: unknown): JsonRecord {
  if (isJsonRecord(template)) {
    return cloneJson(template);
  }

  return {};
}
