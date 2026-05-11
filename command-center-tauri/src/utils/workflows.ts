import { MarkerType, type Edge, type Node } from "@vue-flow/core";
import type { JsonRecord, WorkflowDefinition, WorkflowRunDetail } from "../types/models";
import { statusClassForNode } from "./status";

export function buildGraphNodes(workflow: WorkflowDefinition, detail: WorkflowRunDetail | null): Node[] {
  const definition = workflow.definition ?? {};
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const layout = workflowLayoutNodes(definition);
  const runByNode = new Map((detail?.nodes ?? []).map((run) => [run.node_id, run]));
  return nodes.map((node: JsonRecord, index: number) => {
    const id = String(node.id ?? `step_${index + 1}`);
    const position = layout[id] ?? { x: (index % 4) * 220, y: Math.floor(index / 4) * 90 };
    const run = runByNode.get(id);
    const status = run?.status ?? inferredNodeStatus(node, id, detail);
    const statusLabel = run ? `\n${run.status} a${run.attempt}` : status ? `\n${status}` : "";
    return {
      id,
      type: "workflow",
      position: { x: Number(position.x ?? 0), y: Number(position.y ?? 0) },
      data: {
        label: `${id}\n${node.kind === "task" || !node.kind ? `Task ${node.task_id ?? ""}` : node.kind}${statusLabel}`,
        running: status === "running" || status === "queued",
        status
      },
      class: statusClassForNode(status)
    };
  });
}

export function buildGraphEdges(workflow: WorkflowDefinition): Edge[] {
  const definition = workflow.definition ?? {};
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const nodeIds = new Set(nodes.map((node: JsonRecord) => String(node.id)));
  const edges: Edge[] = [];
  for (const node of nodes) {
    const source = String(node.id ?? "");
    const transitions = node.transitions ?? {};
    for (const key of ["next", "on_success", "on_failure", "on_timeout", "on_reject"]) {
      const target = transitions[key];
      if (target && nodeIds.has(String(target))) {
        edges.push({
          id: `${source}-${key}-${target}`,
          source,
          target: String(target),
          label: key,
          updatable: true,
          markerEnd: MarkerType.ArrowClosed,
          events: {
            // Vue Flow doesn't automatically handle deletion on backspace/delete key unless we configure it,
            // but we can add a delete button or use the built-in edges-change.
          }
        });
      }
    }
    for (const branch of transitions.branches ?? []) {
      if (branch.target && nodeIds.has(String(branch.target))) {
        edges.push({
          id: `${source}-branch-${branch.target}`,
          source,
          target: String(branch.target),
          label: branch.label ?? "branch",
          markerEnd: MarkerType.ArrowClosed
        });
      }
    }
  }
  return edges;
}

export function normalizeWorkflowDefinition(workflow: WorkflowDefinition): WorkflowDefinition {
  const definition = normalizeDefinition(workflow.definition ?? {});
  return { ...workflow, definition };
}

export function workflowLayoutNodes(definition: JsonRecord): JsonRecord {
  const layout = definition.ui?.layout;
  if (!layout || typeof layout !== "object") return {};
  if (layout.nodes && typeof layout.nodes === "object") return layout.nodes;
  return layout;
}

function normalizeDefinition(definition: JsonRecord): JsonRecord {
  const nextDefinition = cloneRecord(definition);
  normalizeLayout(nextDefinition);
  const nodes = Array.isArray(nextDefinition.nodes) ? nextDefinition.nodes : [];
  nextDefinition.nodes = nodes;

  const ids = new Set(nodes.map((node: JsonRecord) => String(node.id)).filter(Boolean));
  const endId = ensureEndNode(nodes, ids);
  const previousStart =
    typeof nextDefinition.start === "string" && ids.has(nextDefinition.start) && nodeKindById(nodes, nextDefinition.start) !== "start"
      ? nextDefinition.start
      : firstNodeId(nodes, (kind) => kind !== "start" && kind !== "end") ?? endId;
  const startId = ensureStartNode(nodes, ids, previousStart, endId);
  routeSuccessTerminalsToEnd(nodes, endId);
  nextDefinition.start = startId;
  return nextDefinition;
}

function normalizeLayout(definition: JsonRecord) {
  const layout = definition.ui?.layout;
  if (!layout || typeof layout !== "object") return;
  const directEntries = Object.entries(layout).filter(([key, value]) => key !== "nodes" && isRecord(value));
  if (directEntries.length === 0) return;
  layout.nodes = isRecord(layout.nodes) ? layout.nodes : {};
  for (const [id, position] of directEntries) {
    if (!layout.nodes[id]) layout.nodes[id] = position;
    delete layout[id];
  }
}

function ensureEndNode(nodes: JsonRecord[], ids: Set<string>): string {
  const existing = firstNodeId(nodes, (kind) => kind === "end");
  if (existing) return existing;
  const id = uniqueNodeId("end", ids);
  nodes.push({ id, kind: "end" });
  return id;
}

function ensureStartNode(nodes: JsonRecord[], ids: Set<string>, previousStart: string, endId: string): string {
  const existing = firstNodeId(nodes, (kind) => kind === "start");
  if (existing) {
    const node = nodes.find((item) => String(item.id) === existing);
    if (node) ensureNextTransition(node, existing === previousStart ? endId : previousStart);
    return existing;
  }
  const id = uniqueNodeId("start", ids);
  nodes.unshift({
    id,
    kind: "start",
    transitions: { next: previousStart === id ? endId : previousStart }
  });
  return id;
}

function routeSuccessTerminalsToEnd(nodes: JsonRecord[], endId: string) {
  for (const node of nodes) {
    if (node.kind === "end" || hasSuccessTransition(node)) continue;
    ensureNextTransition(node, endId);
  }
}

function ensureNextTransition(node: JsonRecord, target: string) {
  node.transitions = isRecord(node.transitions) ? node.transitions : {};
  if (!node.transitions.next) node.transitions.next = target;
}

function hasSuccessTransition(node: JsonRecord): boolean {
  const transitions = node.transitions;
  if (!isRecord(transitions)) return false;
  return Boolean(transitions.next || transitions.on_success || (Array.isArray(transitions.branches) && transitions.branches.length > 0));
}

function inferredNodeStatus(node: JsonRecord, id: string, detail: WorkflowRunDetail | null): string | undefined {
  if (!detail) return undefined;
  if (node.kind === "end" && detail.run.active_node_id === id && detail.run.status === "succeeded") return "succeeded";
  if (node.kind === "start" && detail.nodes.length > 0) return "succeeded";
  return undefined;
}

function firstNodeId(nodes: JsonRecord[], predicate: (kind?: string) => boolean): string | null {
  const node = nodes.find((item) => predicate(typeof item.kind === "string" ? item.kind : undefined));
  return node?.id ? String(node.id) : null;
}

function nodeKindById(nodes: JsonRecord[], id: string): string | undefined {
  const node = nodes.find((item) => String(item.id) === id);
  return typeof node?.kind === "string" ? node.kind : undefined;
}

function uniqueNodeId(base: string, ids: Set<string>): string {
  if (!ids.has(base)) {
    ids.add(base);
    return base;
  }
  for (let index = 2; ; index += 1) {
    const candidate = `${base}_${index}`;
    if (!ids.has(candidate)) {
      ids.add(candidate);
      return candidate;
    }
  }
}

function cloneRecord(value: JsonRecord): JsonRecord {
  return JSON.parse(JSON.stringify(value));
}

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
