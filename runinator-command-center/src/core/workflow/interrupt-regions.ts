import type { JsonRecord, JsonValue } from "../domain/json";
import type { WorkflowDefinition } from "../domain/models";
import { directTransitionKeys, nodeRefId } from "./node-refs";

/** one declared handler: the source that raises it and the node its region starts at. */
export interface InterruptDeclaration {
  source: string;
  handler: string;
  enabled: boolean;
}

/** what a node run belongs to, when it belongs to a handler region rather than the main flow. */
export interface InterruptOrigin {
  source: string;
  handler: string;
  enabled: boolean;
}

function asRecord(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

function asArray(value: unknown): JsonValue[] {
  return Array.isArray(value) ? (value as JsonValue[]) : [];
}

/**
 * the handlers a definition declares.
 *
 * metadata owns the source-to-entry link and whether it is active. the graph owns the linked
 * region's shape, beginning at the handler entry id.
 */
export function interruptDeclarations(
  definition: WorkflowDefinition | null | undefined,
): InterruptDeclaration[] {
  const metadata = asRecord(definition?.definition.metadata);

  return asArray(metadata.interrupts)
    .map((entry) => asRecord(entry))
    .flatMap((entry) => {
      const source = typeof entry.on === "string" ? entry.on : null;
      const handler = typeof entry.handler === "string" ? entry.handler : null;
      return source && handler ? [{ source, handler, enabled: entry.enabled !== false }] : [];
    });
}

/** every node id reachable from `node`, following transitions and branch targets. */
export function nodeTargets(node: JsonRecord): string[] {
  const transitions = asRecord(node.transitions);
  const targets: string[] = [];

  for (const key of directTransitionKeys) {
    const target = nodeRefId(transitions[key]);

    if (target) {
      targets.push(target);
    }
  }

  for (const entry of asArray(transitions.branches)) {
    const target = nodeRefId(asRecord(entry).target);

    if (target) {
      targets.push(target);
    }
  }

  // switch/toggle/percentage carry their arm targets in parameters rather than transitions.
  const parameters = asRecord(node.parameters);

  for (const value of Object.values(parameters)) {
    const direct = nodeRefId(value);

    if (direct) {
      targets.push(direct);
      continue;
    }

    for (const entry of asArray(value)) {
      const target = nodeRefId(asRecord(entry).target) ?? nodeRefId(entry);

      if (target) {
        targets.push(target);
      }
    }
  }

  return targets;
}

/** the outcome of walking one region: the nodes it contains, and the ids it reached that do not exist. */
export interface RegionWalk {
  /** every node id the walk reached, including ones with no matching node. */
  nodes: Set<string>;
  /** the subset of `nodes` no node carries. the backend reports these as broken region members. */
  missing: Set<string>;
}

/** index a definition's nodes by id, skipping entries without a string id. */
export function nodesById(definition: WorkflowDefinition | null | undefined): Map<string, JsonRecord> {
  const byId = new Map<string, JsonRecord>();

  for (const entry of asArray(definition?.definition.nodes)) {
    const node = asRecord(entry);

    if (typeof node.id === "string") {
      byId.set(node.id, node);
    }
  }

  return byId;
}

/**
 * every node reachable from `entry`, following transitions and branch/arm targets.
 *
 * mirrors the backend's `collect_body_region`, including the order it does things in: an id is
 * recorded as a member *before* the node is looked up, so a dangling target is a broken member of
 * the region rather than an edge that silently ends it. that distinction is the whole difference
 * between the validator reporting "region node does not exist" and reporting nothing at all.
 */
export function interruptRegionNodes(byId: Map<string, JsonRecord>, entry: string): RegionWalk {
  const nodes = new Set<string>();
  const missing = new Set<string>();
  const stack = [entry];

  while (stack.length > 0) {
    const id = stack.pop();

    if (id === undefined || nodes.has(id)) {
      continue;
    }

    nodes.add(id);

    const node = byId.get(id);

    if (!node) {
      missing.add(id);
      continue;
    }

    stack.push(...nodeTargets(node));
  }

  return { nodes, missing };
}

/**
 * map every node belonging to an interrupt handler region to the interrupt it answers.
 *
 * keyed on the *node*, deliberately. a handler cursor is ephemeral — it retires the moment its
 * region ends — so by the time anyone looks at a finished run's timeline the cursor is gone from
 * run state. region membership is a static property of the graph, so it still explains rows the
 * cursor can no longer account for. this mirrors the backend's `interrupt_region_nodes`.
 */
export function interruptRegionOrigins(
  definition: WorkflowDefinition | null | undefined,
): Map<string, InterruptOrigin> {
  const origins = new Map<string, InterruptOrigin>();
  const declarations = interruptDeclarations(definition);

  if (declarations.length === 0) {
    return origins;
  }

  const byId = nodesById(definition);

  for (const declaration of declarations) {
    const { nodes, missing } = interruptRegionNodes(byId, declaration.handler);

    for (const id of nodes) {
      // a node that does not exist has no timeline row to attribute, and the first declaration to
      // claim a shared node keeps it -- both matching the walk this replaced.
      if (missing.has(id) || origins.has(id)) {
        continue;
      }

      origins.set(id, {
        source: declaration.source,
        handler: declaration.handler,
        enabled: declaration.enabled,
      });
    }
  }

  return origins;
}
