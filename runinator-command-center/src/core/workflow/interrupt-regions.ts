import type { JsonRecord, JsonValue } from "../domain/json";
import type { WorkflowDefinition } from "../domain/models";
import { directTransitionKeys, nodeRefId } from "./index";

/** one declared handler: the source that raises it and the node its region starts at. */
export interface InterruptDeclaration {
  source: string;
  handler: string;
}

/** what a node run belongs to, when it belongs to a handler region rather than the main flow. */
export interface InterruptOrigin {
  source: string;
  handler: string;
}

function asRecord(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

function asArray(value: unknown): JsonValue[] {
  return Array.isArray(value) ? (value as JsonValue[]) : [];
}

/** the handlers a definition declares, read from `metadata.interrupts`. */
export function interruptDeclarations(
  definition: WorkflowDefinition | null | undefined,
): InterruptDeclaration[] {
  const metadata = asRecord(definition?.definition.metadata);

  return asArray(metadata.interrupts)
    .map((entry) => asRecord(entry))
    .flatMap((entry) => {
      const source = typeof entry.on === "string" ? entry.on : null;
      const handler = typeof entry.handler === "string" ? entry.handler : null;
      return source && handler ? [{ source, handler }] : [];
    });
}

/** every node id reachable from `node`, following transitions and branch targets. */
function outgoing(node: JsonRecord): string[] {
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

  const byId = new Map<string, JsonRecord>();

  for (const entry of asArray(definition?.definition.nodes)) {
    const node = asRecord(entry);

    if (typeof node.id === "string") {
      byId.set(node.id, node);
    }
  }

  for (const declaration of declarations) {
    const stack = [declaration.handler];

    while (stack.length > 0) {
      const id = stack.pop();

      if (id === undefined || origins.has(id)) {
        continue;
      }

      const node = byId.get(id);

      if (!node) {
        continue;
      }

      origins.set(id, { source: declaration.source, handler: declaration.handler });
      stack.push(...outgoing(node));
    }
  }

  return origins;
}
