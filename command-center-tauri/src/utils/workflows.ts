import type { Edge, Node } from "@vue-flow/core";
import type { JsonRecord, WorkflowDefinition, WorkflowRunDetail } from "../types/models";
import { statusClassForNode } from "./status";

export function buildGraphNodes(workflow: WorkflowDefinition, detail: WorkflowRunDetail | null): Node[] {
  const definition = workflow.definition ?? {};
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const layout = definition.ui?.layout?.nodes ?? {};
  const runByNode = new Map((detail?.nodes ?? []).map((run) => [run.node_id, run]));
  return nodes.map((node: JsonRecord, index: number) => {
    const id = String(node.id ?? `step_${index + 1}`);
    const position = layout[id] ?? { x: (index % 4) * 220, y: Math.floor(index / 4) * 90 };
    const run = runByNode.get(id);
    const status = run?.status;
    return {
      id,
      position: { x: Number(position.x ?? 0), y: Number(position.y ?? 0) },
      data: { label: `${id}\n${node.kind === "task" || !node.kind ? `Task ${node.task_id ?? ""}` : node.kind}${run ? `\n${run.status} a${run.attempt}` : ""}` },
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
        edges.push({ id: `${source}-${key}-${target}`, source, target: String(target), label: key });
      }
    }
    for (const branch of transitions.branches ?? []) {
      if (branch.target && nodeIds.has(String(branch.target))) {
        edges.push({ id: `${source}-branch-${branch.target}`, source, target: String(branch.target), label: branch.label ?? "branch" });
      }
    }
  }
  return edges;
}
