// client-side validation for the four workflow header declarations.
//
// two things make this load-bearing rather than cosmetic. saving round-trips the definition through
// rexrap -- decompile to text, recompile on the server -- so a header the decompiler cannot render is
// either rejected at save or, worse, silently rewritten into something else. and the backend
// validates interrupts only: watches, concurrency, and correlation are read fail-open, so a broken
// one is simply a declaration that never does anything.
//
// the interrupt rules below transcribe `validate_interrupt_handlers`
// (runinator-workflows/src/validation.rs) one for one. the extra multi-path warning has no backend
// counterpart: it is a decompile limit (`emit_region` refuses a node reached twice), not a rule
// about what the runtime can execute.

import type { JsonRecord, JsonValue } from "../domain/json";
import type { WorkflowValidationIssue } from "../domain/models";
import { findNodeKindMetadata, isNodeCatalogLoaded } from "./catalog-registry";
import { readWorkflowHeader } from "./header-metadata";
import type { WatchDeclaration } from "./header-metadata";
import { interruptRegionNodes, nodeTargets } from "./interrupt-regions";
import type { InterruptDeclaration } from "./interrupt-regions";
import { displayValue } from "../utils/values";

/** the nodeId carried by an issue that belongs to the workflow rather than to any one node. */
export const HEADER_ISSUE_NODE_ID = "workflow";

/** every source the backend knows. an unknown one is a closed grammar alternative, so it 400s. */
const INTERRUPT_SOURCES = new Set([
  "external",
  "orphan_signal",
  "wake",
  "timeout",
  "retry",
  "failure",
  "resolved",
  "child",
]);

/** the comparison keys `Decompiler::cond` can render, mirroring its `CMP_OPS`. */
const CONDITION_COMPARISONS = new Set([
  "equals",
  "not_equals",
  "greater_than_or_equal",
  "less_than_or_equal",
  "greater_than",
  "less_than",
  "contains",
  "in",
  "starts_with",
  "ends_with",
]);

/** the rexrap `ident` production, which is what a `watch -> target` has to be spelled as. */
const REXRAP_IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;

function asRecord(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

/** a node's kind as written. unlike the graph builder's coercion, an unrecognized kind stays itself
 * here -- the catalog lookup then misses, which is exactly the "not allowed in a region" answer. */
function kindOf(node: JsonRecord | undefined): string {
  return displayValue(node?.kind) || "action";
}

function error(
  message: string,
  nodeId = HEADER_ISSUE_NODE_ID,
  interruptHandlerId?: string,
): WorkflowValidationIssue {
  return { severity: "error", nodeId, message, interruptHandlerId };
}

function warning(
  message: string,
  nodeId = HEADER_ISSUE_NODE_ID,
  interruptHandlerId?: string,
): WorkflowValidationIssue {
  return { severity: "warning", nodeId, message, interruptHandlerId };
}

/**
 * validate the header declarations of a definition.
 *
 * appended to `validateWorkflowIssues`, so these land in the canvas diagnostics table and -- for
 * the node-scoped ones -- as a badge on the offending node, with no extra plumbing.
 *
 * the two halves are also exported separately because interrupts and the remaining declarations
 * live in different inspector panels, and a panel badge that counted the other panel's problems
 * would send the user to the wrong tab.
 */
export function headerIssues(definition: JsonRecord): WorkflowValidationIssue[] {
  return [...interruptIssues(definition), ...declarationIssues(definition)];
}

/** the interrupt half: everything `validate_interrupt_handlers` checks, plus the decompile warning. */
export function interruptIssues(definition: JsonRecord): WorkflowValidationIssue[] {
  const header = readWorkflowHeader(definition);
  const issues: WorkflowValidationIssue[] = [];
  const byId = definitionNodesById(definition);
  const start = typeof definition.start === "string" ? definition.start : "";
  const reachable = start ? interruptRegionNodes(byId, start).nodes : new Set<string>();

  pushInterruptIssues(issues, header.interrupts, byId, reachable);
  const linked = new Set(header.interrupts.map((entry) => entry.handler));

  for (const [id, node] of byId) {
    if (kindOf(node) === "interrupt" && !linked.has(id)) {
      issues.push(
        error(
          `Interrupt entry '${id}' is not linked by workflow metadata; delete the orphaned region or restore its handler declaration`,
          id,
        ),
      );
    }
  }

  return issues;
}

/** the rest of the header: watch guards, the concurrency policy, and the correlation key. */
export function declarationIssues(definition: JsonRecord): WorkflowValidationIssue[] {
  const header = readWorkflowHeader(definition);
  const issues: WorkflowValidationIssue[] = [];
  const byId = definitionNodesById(definition);

  for (const watch of header.watches) {
    pushWatchIssues(issues, watch, byId);
  }

  if (header.concurrency) {
    const { maxConcurrentRuns, onConflict } = header.concurrency;

    if (maxConcurrentRuns < 1) {
      // rexrap refuses `concurrency 0`, and the decompiler drops the whole header -- policy included --
      // rather than emitting it, so the save would quietly discard what was set here.
      issues.push(
        error(
          `Concurrency limit must be at least 1 (omit the header for an unlimited workflow); ${String(maxConcurrentRuns)} would be dropped on save`,
        ),
      );
    } else if (onConflict === "allow") {
      issues.push(
        warning(
          "Concurrency policy 'allow' never declines a firing, so the limit has no effect",
        ),
      );
    }
  }

  return issues;
}

function definitionNodesById(definition: JsonRecord): Map<string, JsonRecord> {
  const byId = new Map<string, JsonRecord>();

  for (const entry of Array.isArray(definition.nodes) ? definition.nodes : []) {
    const node = asRecord(entry);
    const id = displayValue(node.id);

    if (id) {
      byId.set(id, node);
    }
  }

  return byId;
}

function pushInterruptIssues(
  issues: WorkflowValidationIssue[],
  declarations: InterruptDeclaration[],
  byId: Map<string, JsonRecord>,
  reachable: Set<string>,
): void {
  const seenSources = new Set<string>();
  const claimed = new Map<string, string>();
  // the kind rules need the catalog. reporting them against an unloaded catalog would flag every
  // region as unsupported, so they are skipped until it arrives rather than guessed at.
  const kindsKnown = isNodeCatalogLoaded();

  for (const declaration of declarations) {
    const { source, handler } = declaration;
    const label = `Interrupt handler '${handler}'`;
    const handlerError = (message: string, nodeId = HEADER_ISSUE_NODE_ID) =>
      error(message, nodeId, handler);
    const handlerWarning = (message: string, nodeId = HEADER_ISSUE_NODE_ID) =>
      warning(message, nodeId, handler);

    if (!INTERRUPT_SOURCES.has(source)) {
      issues.push(
        handlerError(`${label} declares unknown source '${source}'; the workflow will not compile`),
      );
    }

    if (seenSources.has(source)) {
      issues.push(handlerError(`${label}: source '${source}' already has a handler; one handler per source`));
    }

    seenSources.add(source);

    const entry = byId.get(handler);

    if (!entry) {
      issues.push(handlerError(`${label} does not exist`));
      continue;
    }

    if (kindsKnown && !findNodeKindMetadata(kindOf(entry))?.runnable_entry) {
      issues.push(
        handlerError(`${label} is a ${kindOf(entry)} node, which cannot start a region`, handler),
      );
    }

    if (reachable.has(handler)) {
      issues.push(
        handlerError(`${label} is reachable from the workflow start; a region must be entered only by its interrupt`, handler),
      );
    }

    const { nodes, missing } = interruptRegionNodes(byId, handler);
    const converging = convergingNodes(byId, nodes);
    let sawResume = false;

    for (const id of nodes) {
      if (missing.has(id)) {
        issues.push(handlerError(`${label}: region node '${id}' does not exist`, handler));
        continue;
      }

      const kind = kindOf(byId.get(id));
      sawResume ||= kind === "resume";

      if (kindsKnown && !findNodeKindMetadata(kind)?.handler_safe) {
        issues.push(handlerError(`${label}: '${id}' is a ${kind} node, which is not allowed inside a handler region`, id));
      }

      if (id !== handler && reachable.has(id)) {
        issues.push(handlerError(`${label}: region node '${id}' is also reachable from the workflow start`, id));
      }

      const owner = claimed.get(id);

      if (owner !== undefined && owner !== handler) {
        issues.push(handlerError(`${label}: region node '${id}' already belongs to handler '${owner}'`, id));
      }

      claimed.set(id, handler);
    }

    if (!sawResume) {
      issues.push(
        handlerError(`${label}: region never reaches a resume node, so it can never return control`, handler),
      );
    }

    for (const [outsideId, node] of byId) {
      if (nodes.has(outsideId)) {
        continue;
      }

      for (const target of nodeTargets(node)) {
        if (nodes.has(target)) {
          issues.push(handlerError(`${label}: '${outsideId}' transitions into the region at '${target}'`, outsideId));
        }
      }
    }

    for (const id of converging) {
      issues.push(
        handlerWarning(
          `${label}: '${id}' is reached by more than one path inside the region, which cannot be written as rexrap`,
          id,
        ),
      );
    }
  }
}

/**
 * region members that more than one member points at.
 *
 * `emit_region` walks a region as a structured statement list and errors on any node it reaches
 * twice, so a diamond inside a handler is a hard decompile failure even though the runtime would
 * execute it happily. one in-degree pass over the region answers it for every member at once.
 */
function convergingNodes(byId: Map<string, JsonRecord>, region: Set<string>): string[] {
  const inDegree = new Map<string, number>();

  for (const memberId of region) {
    const node = byId.get(memberId);

    if (!node) {
      continue;
    }

    for (const target of nodeTargets(node)) {
      if (region.has(target)) {
        inDegree.set(target, (inDegree.get(target) ?? 0) + 1);
      }
    }
  }

  return [...inDegree].flatMap(([id, count]) => (count > 1 ? [id] : []));
}

function pushWatchIssues(
  issues: WorkflowValidationIssue[],
  watch: WatchDeclaration,
  byId: Map<string, JsonRecord>,
): void {
  const label = `Watch guard -> '${watch.handler}'`;

  // `end`/`fail` are spelled `done`/`fail` in rexrap and always exist as targets.
  if (watch.handler !== "end" && watch.handler !== "fail") {
    if (!byId.has(watch.handler)) {
      issues.push(error(`${label}: handler node does not exist`));
    } else if (!REXRAP_IDENT.test(watch.handler)) {
      issues.push(
        error(`${label}: handler id is not a legal rexrap identifier, so the workflow will not compile`),
      );
    }
  }

  if (!isRenderableCondition(watch.condition)) {
    issues.push(
      error(`${label}: condition is not a shape rexrap can express, so it would be lost on save`),
    );
  }
}

/**
 * can `Decompiler::cond` render this condition?
 *
 * a direct transcription of that function's branches. anything it rejects is a hard decompile
 * failure, which takes down the rexrap pane and the save with it.
 */
export function isRenderableCondition(value: JsonValue): boolean {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }

  const object = value as Record<string, JsonValue>;

  if (Array.isArray(object.all)) {
    return object.all.every((item) => isRenderableCondition(item));
  }

  if (Array.isArray(object.any)) {
    return object.any.every((item) => isRenderableCondition(item));
  }

  if (Object.hasOwn(object, "not")) {
    return isRenderableCondition(object.not);
  }

  const hasLeft = Object.hasOwn(object, "value") || Object.hasOwn(object, "left");

  if (!hasLeft) {
    return false;
  }

  if (Object.hasOwn(object, "exists")) {
    return true;
  }

  for (const key of Object.keys(object)) {
    if (CONDITION_COMPARISONS.has(key)) {
      return true;
    }
  }

  // the bare truthiness form: exactly `{ value: <expr> }` and nothing else.
  return Object.keys(object).length === 1 && Object.hasOwn(object, "value");
}
