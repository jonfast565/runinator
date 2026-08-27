// the workflow *header*: the four declarations that belong to a workflow rather than to any one
// node -- interrupt handlers, watch guards, the concurrency policy, and the correlation key.
//
// they live under `definition.metadata`, written there by the rexrap lowerer and read back by the
// decompiler on every save, so this module is the one place that knows their wire shape. read into
// a `WorkflowHeader`, edit that, apply it back. the `on` <-> `source` rename for interrupts lives
// here and nowhere else.

import type { JsonRecord, JsonValue } from "../domain/json";
import type { InterruptDeclaration } from "./interrupt-regions";

/** a workflow-level guard: when `condition` holds, the run jumps to `handler`. fires once per run. */
export interface WatchDeclaration {
  condition: JsonValue;
  handler: string;
}

/** how many runs of this workflow may overlap, and what a trigger does when it cannot start one. */
export interface ConcurrencyHeader {
  maxConcurrentRuns: number;
  onConflict: string;
}

export interface WorkflowHeader {
  interrupts: InterruptDeclaration[];
  watches: WatchDeclaration[];
  /** null means no header at all, which the runtime reads as unlimited. */
  concurrency: ConcurrencyHeader | null;
  /** the expression stamped write-once onto a run's correlation key; null means no key. */
  correlation: JsonValue | null;
}

function asRecord(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

function asArray(value: unknown): JsonValue[] {
  return Array.isArray(value) ? (value as JsonValue[]) : [];
}

export function emptyWorkflowHeader(): WorkflowHeader {
  return { interrupts: [], watches: [], concurrency: null, correlation: null };
}

/** true when nothing in the header would be written, so every key can be dropped. */
export function isEmptyWorkflowHeader(header: WorkflowHeader): boolean {
  return (
    header.interrupts.length === 0 &&
    header.watches.length === 0 &&
    header.concurrency === null &&
    header.correlation === null
  );
}

/** read the four header declarations out of a workflow definition's metadata. */
export function readWorkflowHeader(definition: JsonRecord): WorkflowHeader {
  const metadata = asRecord(definition.metadata);

  return {
    interrupts: readInterrupts(metadata),
    watches: readWatches(metadata),
    concurrency: readConcurrency(metadata),
    // `correlation` is a bare lowered expression, so anything but an absent key is a value --
    // including `false` or `0`, which a truthiness check would drop.
    correlation: metadata.correlation === undefined ? null : (metadata.correlation as JsonValue),
  };
}

function readInterrupts(metadata: JsonRecord): InterruptDeclaration[] {
  return asArray(metadata.interrupts)
    .map((entry) => asRecord(entry))
    .flatMap((entry) => {
      const source = typeof entry.on === "string" ? entry.on : null;
      const handler = typeof entry.handler === "string" ? entry.handler : null;
      // a half-written entry is dropped rather than surfaced: the backend decodes the whole array
      // or none of it, so a shape it would reject must not round-trip through the editor either.
      const intervalSeconds =
        typeof entry.interval_seconds === "number" && Number.isFinite(entry.interval_seconds)
          ? entry.interval_seconds
          : undefined;
      return source !== null && handler !== null
        ? [{ source, handler, enabled: entry.enabled !== false, intervalSeconds }]
        : [];
    });
}

function readWatches(metadata: JsonRecord): WatchDeclaration[] {
  return asArray(metadata.watches)
    .map((entry) => asRecord(entry))
    .flatMap((entry) => {
      const handler = typeof entry.handler === "string" ? entry.handler : null;
      return handler === null
        ? []
        : [{ condition: (entry.condition ?? null) as JsonValue, handler }];
    });
}

function readConcurrency(metadata: JsonRecord): ConcurrencyHeader | null {
  if (metadata.concurrency === undefined || metadata.concurrency === null) {
    return null;
  }

  const entry = asRecord(metadata.concurrency);
  const max = Number(entry.max_concurrent_runs ?? 0);

  return {
    maxConcurrentRuns: Number.isFinite(max) ? max : 0,
    onConflict: typeof entry.on_conflict === "string" ? entry.on_conflict : "allow",
  };
}

/**
 * write a header back into a definition's `metadata`, in place.
 *
 * an empty section **deletes** its key rather than writing `[]` or `{}`, mirroring the lowerer's
 * `if !x.is_empty()` guards -- a definition that came from rexrap never carries an empty one, and
 * writing one would make every save produce a diff against its own decompiled output. keys this
 * module does not own (`rexrap`, `triggers`, `notifications`, `functions`) are left untouched.
 */
export function applyWorkflowHeader(definition: JsonRecord, header: WorkflowHeader): void {
  const owned: JsonRecord = {
    interrupts:
      header.interrupts.length > 0
        ? header.interrupts.map((entry) => ({
            on: entry.source,
            handler: entry.handler,
            ...(entry.intervalSeconds === undefined
              ? {}
              : { interval_seconds: entry.intervalSeconds }),
            ...(entry.enabled ? {} : { enabled: false }),
          }))
        : undefined,
    watches:
      header.watches.length > 0
        ? header.watches.map((entry) => ({ condition: entry.condition, handler: entry.handler }))
        : undefined,
    concurrency: header.concurrency
      ? {
          max_concurrent_runs: header.concurrency.maxConcurrentRuns,
          on_conflict: header.concurrency.onConflict,
        }
      : undefined,
    correlation: header.correlation ?? undefined,
  };

  // rebuild rather than mutate: an absent section has to leave no key behind, and rebuilding drops
  // it without a dynamic `delete` per key.
  const metadata: JsonRecord = Object.fromEntries([
    ...Object.entries(asRecord(definition.metadata)).filter(([key]) => !(key in owned)),
    ...Object.entries(owned).filter(([, value]) => value !== undefined),
  ]);

  // only materialize the metadata object if something is actually in it, so an untouched workflow
  // does not gain an empty `metadata: {}` the moment the header panel is opened.
  if (Object.keys(metadata).length > 0) {
    definition.metadata = metadata;
    return;
  }

  delete definition.metadata;
}
