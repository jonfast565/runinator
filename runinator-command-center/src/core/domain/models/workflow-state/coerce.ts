import {
  asJsonObject,
  asJsonValue,
  isJsonRecord,
  type JsonObject,
  type JsonValue,
} from "../../json";
import type { WorkflowNodeKind } from "../workflow/node-kind";
import type { CompensationFrame } from "./compensation-frame";
import type { ControlFrame } from "./control-frame";
import type { DebugFrame } from "./debug-frame";
import type { DebugMode } from "./debug-mode";
import type { LoopFrame } from "./loop-frame";
import type { MapFrame } from "./map-frame";
import type { ParallelFrame } from "./parallel-frame";
import type { RaceFrame } from "./race-frame";
import type { TryFrame } from "./try-frame";
import type { WorkflowRunState } from "./workflow-run-state";

function stringArray(value: JsonValue | undefined): string[] | undefined {
  if (!Array.isArray(value)) {
    return undefined;
  }

  return value.filter((entry): entry is string => typeof entry === "string");
}

function debugMode(value: JsonValue | undefined): DebugMode | undefined {
  return value === "step_all" || value === "breakpoints" ? value : undefined;
}

function optionalJsonValue(value: unknown): JsonValue | undefined {
  if (value === undefined) {
    return undefined;
  }

  return asJsonValue(value);
}

function coerceRecord(value: unknown): JsonObject {
  if (value === undefined || value === null) {
    return {};
  }

  return asJsonObject(asJsonValue(value));
}

function workflowNodeKind(value: JsonValue | undefined): WorkflowNodeKind | null | undefined {
  return typeof value === "string" ? (value as WorkflowNodeKind) : null;
}

export function coerceControlFrame(value: unknown): ControlFrame | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }

  const record = coerceRecord(value);

  if (Object.keys(record).length === 0) {
    return undefined;
  }

  return {
    pause_requested: typeof record.pause_requested === "boolean" ? record.pause_requested : undefined,
  };
}

export function coerceDebugFrame(value: unknown): DebugFrame | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }

  const record = coerceRecord(value);

  if (Object.keys(record).length === 0) {
    return undefined;
  }

  return {
    enabled: typeof record.enabled === "boolean" ? record.enabled : undefined,
    mode: debugMode(record.mode),
    breakpoints: stringArray(record.breakpoints),
    paused: typeof record.paused === "boolean" ? record.paused : undefined,
    step_requested: typeof record.step_requested === "boolean" ? record.step_requested : undefined,
    one_shot_breakpoint:
      typeof record.one_shot_breakpoint === "string" ? record.one_shot_breakpoint : null,
    current_node_id: typeof record.current_node_id === "string" ? record.current_node_id : null,
    current_node_kind: workflowNodeKind(record.current_node_kind),
    input_json: optionalJsonValue(record.input_json),
    context_json: optionalJsonValue(record.context_json),
    last_output_json: optionalJsonValue(record.last_output_json),
  };
}

export function coerceLoopFrame(value: unknown): LoopFrame | undefined {
  const record = coerceRecord(value);

  if (Object.keys(record).length === 0) {
    return undefined;
  }

  return {
    index: typeof record.index === "number" ? record.index : undefined,
    item: optionalJsonValue(record.item),
    return_to: typeof record.return_to === "string" ? record.return_to : undefined,
  };
}

export function coerceParallelFrame(value: unknown): ParallelFrame | undefined {
  const record = coerceRecord(value);

  if (typeof record.node_id !== "string") {
    return undefined;
  }

  return {
    node_id: record.node_id,
    remaining: stringArray(record.remaining),
  };
}

export function coerceRaceFrame(value: unknown): RaceFrame | undefined {
  const record = coerceRecord(value);

  if (typeof record.node_id !== "string") {
    return undefined;
  }

  return {
    node_id: record.node_id,
    remaining: stringArray(record.remaining),
  };
}

export function coerceTryFrame(value: unknown): TryFrame | undefined {
  const record = coerceRecord(value);

  if (typeof record.node_id !== "string" || typeof record.phase !== "string") {
    return undefined;
  }

  return {
    node_id: record.node_id,
    phase: record.phase,
    pending_status: typeof record.pending_status === "string" ? record.pending_status : null,
    pending_output: optionalJsonValue(record.pending_output) ?? null,
  };
}

export function coerceCompensationFrame(value: unknown): CompensationFrame | undefined {
  const record = coerceRecord(value);

  if (Object.keys(record).length === 0) {
    return undefined;
  }

  return {
    remaining: stringArray(record.remaining),
    active_run_id: typeof record.active_run_id === "string" ? record.active_run_id : null,
  };
}

export function coerceMapFrame(value: unknown): MapFrame | undefined {
  const record = coerceRecord(value);

  if (typeof record.node_id !== "string" || typeof record.target !== "string") {
    return undefined;
  }

  return {
    node_id: record.node_id,
    target: record.target,
    items: Array.isArray(record.items) ? record.items.map((item) => asJsonValue(item)) : undefined,
    concurrency: typeof record.concurrency === "number" ? record.concurrency : undefined,
    next_index: typeof record.next_index === "number" ? record.next_index : undefined,
    done: typeof record.done === "number" ? record.done : undefined,
    item: optionalJsonValue(record.item) ?? null,
    index: typeof record.index === "number" ? record.index : undefined,
  };
}

/** parse a run `state` blob into typed frames; returns null when value is not an object. */
export function coerceWorkflowRunState(value: unknown): WorkflowRunState | null {
  if (!isJsonRecord(value)) {
    return null;
  }

  const record = value;

  return {
    control: coerceControlFrame(record.control),
    debug: coerceDebugFrame(record.debug),
    loop: coerceLoopFrame(record.loop),
    parallel: coerceParallelFrame(record.parallel),
    map: coerceMapFrame(record.map),
    race: coerceRaceFrame(record.race),
    try: coerceTryFrame(record.try),
    compensation: coerceCompensationFrame(record.compensation),
    run_metadata: optionalJsonValue(record.run_metadata),
    watch_fired: record.watch_fired === true,
  };
}
