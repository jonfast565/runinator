import { MarkerType, type Edge, type Node } from "@vue-flow/core";
import type {
  JsonRecord,
  ScheduledTask,
  WorkflowDefinition,
  WorkflowDirectTransitionKey,
  WorkflowEditorEdgeData,
  WorkflowEditorNodeRecord,
  WorkflowNodeKind,
  WorkflowRunDetail
} from "../types/models";
import { statusClassForNode } from "./status";

export const workflowNodeKinds: WorkflowNodeKind[] = [
  "task",
  "approval",
  "loop",
  "condition",
  "wait",
  "switch",
  "parallel",
  "join",
  "try",
  "map",
  "race",
  "emit",
  "subflow"
];

export const directTransitionKeys: WorkflowDirectTransitionKey[] = ["next", "on_success", "on_failure", "on_timeout", "on_reject"];

export function buildGraphNodes(workflow: WorkflowDefinition, detail: WorkflowRunDetail | null, tasks: ScheduledTask[] = []): Node[] {
  const definition = workflow.definition ?? {};
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const layout = workflowLayoutNodes(definition);
  const runByNode = new Map((detail?.nodes ?? []).map((run) => [run.node_id, run]));
  const taskById = new Map(tasks.filter((task) => task.id !== null).map((task) => [task.id, task]));
  return nodes.map((node: JsonRecord, index: number) => {
    const id = String(node.id ?? `step_${index + 1}`);
    const position = layout[id] ?? { x: (index % 4) * 220, y: Math.floor(index / 4) * 90 };
    const run = runByNode.get(id);
    const status = run?.status ?? inferredNodeStatus(node, id, detail);
    const kind = workflowNodeKind(node.kind);
    const task = kind === "task" ? taskById.get(Number(node.task_id ?? 0)) : undefined;
    return {
      id,
      type: "workflow",
      position: { x: Number(position.x ?? 0), y: Number(position.y ?? 0) },
      data: {
        title: id,
        kind,
        summary: nodeSummary(node, task),
        statusLabel: run ? `${run.status} a${run.attempt}` : status,
        approvalPrompt: approvalPrompt(node, run?.state),
        running: status === "running" || status === "queued",
        status,
        protected: kind === "start" || kind === "end"
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
    for (const key of directTransitionKeys) {
      const target = nodeRefId(transitions[key]);
      if (target && nodeIds.has(target)) {
        edges.push(graphEdge(source, target, key, { kind: "direct", transitionKey: key, editable: true }));
      }
    }
    for (const [index, branch] of (transitions.branches ?? []).entries()) {
      const target = nodeRefId(branch.target);
      if (target && nodeIds.has(target)) {
        edges.push(graphEdge(source, target, branch.label ?? `branch ${index + 1}`, { kind: "branch", branchIndex: index, editable: true }));
      }
    }
    edges.push(...controlFlowEdges(node, nodeIds));
  }
  return edges;
}

export function createWorkflowNode(kind: WorkflowNodeKind, nodes: JsonRecord[], taskId = 1): WorkflowEditorNodeRecord {
  const id = uniqueWorkflowNodeId(nodes, kind);
  const node: WorkflowEditorNodeRecord = {
    id,
    kind,
    parameters: {},
    transitions: {}
  };
  switch (kind) {
    case "task":
      node.task_id = taskId;
      node.retry = { max_attempts: 1 };
      break;
    case "approval":
      node.parameters = { approval_type: "generic", prompt: "Approval required" };
      node.transitions = { on_success: nodeRef("end"), on_reject: nodeRef("end") };
      break;
    case "loop":
      node.parameters = { items: [], target: nodeRef("end") };
      node.max_iterations = 10;
      break;
    case "condition":
      node.condition = {};
      node.transitions = {
        branches: [{ when: { value: valueRef("input", ["approved"]), equals: true }, target: nodeRef("end") }],
        next: nodeRef("end")
      };
      break;
    case "wait":
      node.wait = { seconds: 60 };
      break;
    case "switch":
      node.parameters = { value: valueRef("input", ["mode"]), cases: [], default: nodeRef("end") };
      break;
    case "parallel":
    case "race":
      node.parameters = { branches: [] };
      break;
    case "join":
      node.parameters = { wait_for: [], mode: "all" };
      break;
    case "try":
      node.parameters = {};
      break;
    case "map":
      node.parameters = { items: [], target: nodeRef("end"), concurrency: 1 };
      break;
    case "emit":
      node.parameters = { event_type: "workflow.event", data: {} };
      break;
    case "subflow":
      node.subflow_id = 0;
      break;
  }
  return node;
}

export function uniqueWorkflowNodeId(nodes: JsonRecord[], base: string): string {
  return uniqueNodeId(base.replace(/[^a-zA-Z0-9_]+/g, "_") || "node", new Set(nodes.map((node) => String(node.id)).filter(Boolean)));
}

export function addDirectTransition(node: JsonRecord, target: string, preferredKey?: string | null): WorkflowDirectTransitionKey {
  const key = directTransitionKeys.includes(preferredKey as WorkflowDirectTransitionKey) ? (preferredKey as WorkflowDirectTransitionKey) : firstAvailableTransition(node);
  node.transitions = isRecord(node.transitions) ? node.transitions : {};
  node.transitions[key] = nodeRef(target);
  return key;
}

export function removeEditableEdge(node: JsonRecord, edge: Edge): boolean {
  const data = edge.data as WorkflowEditorEdgeData | undefined;
  if (!data?.editable || !isRecord(node.transitions)) return false;
  if (data.kind === "direct" && data.transitionKey && nodeRefId(node.transitions[data.transitionKey]) === edge.target) {
    delete node.transitions[data.transitionKey];
    return true;
  }
  if (data.kind === "branch" && typeof data.branchIndex === "number" && Array.isArray(node.transitions.branches)) {
    const branch = node.transitions.branches[data.branchIndex];
    if (nodeRefId(branch?.target) !== edge.target) return false;
    node.transitions.branches.splice(data.branchIndex, 1);
    return true;
  }
  return false;
}

export function setConditionBranch(node: JsonRecord, index: number, when: JsonRecord, target: string) {
  node.transitions = isRecord(node.transitions) ? node.transitions : {};
  node.transitions.branches = Array.isArray(node.transitions.branches) ? node.transitions.branches : [];
  node.transitions.branches[index] = { when, target: nodeRef(target) };
}

export function removeConditionBranch(node: JsonRecord, index: number) {
  if (!isRecord(node.transitions) || !Array.isArray(node.transitions.branches)) return;
  node.transitions.branches.splice(index, 1);
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
    transitions: { next: nodeRef(previousStart === id ? endId : previousStart) }
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
  if (!node.transitions.next) node.transitions.next = nodeRef(target);
}

function hasSuccessTransition(node: JsonRecord): boolean {
  const transitions = node.transitions;
  return Boolean(
    (isRecord(transitions) &&
      (nodeRefId(transitions.next) || nodeRefId(transitions.on_success) || (Array.isArray(transitions.branches) && transitions.branches.length > 0))) ||
      controlFlowTargetValues(node).length > 0
  );
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

function graphEdge(source: string, target: string, label: string, data: WorkflowEditorEdgeData): Edge {
  return {
    id: edgeId(source, target, label, data),
    source,
    target,
    label,
    data,
    updatable: data.editable,
    markerEnd: MarkerType.ArrowClosed
  };
}

function edgeId(source: string, target: string, label: string, data: WorkflowEditorEdgeData): string {
  return [source, data.kind, data.transitionKey ?? data.parameterKey ?? data.branchIndex ?? label, data.parameterIndex ?? "", target]
    .map((part) => encodeURIComponent(String(part)))
    .join(":");
}

function controlFlowEdges(node: JsonRecord, nodeIds: Set<string>): Edge[] {
  const source = String(node.id ?? "");
  return controlFlowTargetValues(node)
    .filter(({ target }) => nodeIds.has(target))
    .map(({ target, label, parameterKey, parameterIndex }) =>
      graphEdge(source, target, label, { kind: "control", parameterKey, parameterIndex, editable: false })
    );
}

function controlFlowTargetValues(node: JsonRecord): Array<{ target: string; label: string; parameterKey?: string; parameterIndex?: number }> {
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  switch (node.kind) {
    case "switch": {
      const cases = Array.isArray(parameters.cases) ? parameters.cases : [];
      const targets: Array<{ target: string; label: string; parameterKey?: string; parameterIndex?: number }> = cases
        .filter(isRecord)
        .map((item, index) => ({ target: nodeRefId(item.target), label: item.label ? String(item.label) : `case ${index + 1}`, parameterKey: "cases", parameterIndex: index }))
        .filter((item): item is { target: string; label: string; parameterKey: string; parameterIndex: number } => Boolean(item.target));
      const defaultTarget = nodeRefId(parameters.default);
      if (defaultTarget) targets.push({ target: defaultTarget, label: "default", parameterKey: "default" });
      return targets;
    }
    case "parallel":
      return nodeRefArray(parameters.branches).map((target, parameterIndex) => ({ target, label: "branch", parameterKey: "branches", parameterIndex }));
    case "join":
      return nodeRefArray(parameters.wait_for).map((target, parameterIndex) => ({ target, label: "wait_for", parameterKey: "wait_for", parameterIndex }));
    case "try": {
      const targets: Array<{ target: string; label: string; parameterKey: string }> = [];
      const body = nodeRefId(parameters.body);
      const catchTarget = nodeRefId(parameters.catch);
      const finallyTarget = nodeRefId(parameters.finally);
      if (body) targets.push({ target: body, label: "body", parameterKey: "body" });
      if (catchTarget) targets.push({ target: catchTarget, label: "catch", parameterKey: "catch" });
      if (finallyTarget) targets.push({ target: finallyTarget, label: "finally", parameterKey: "finally" });
      return targets;
    }
    case "map":
      return nodeRefId(parameters.target) ? [{ target: nodeRefId(parameters.target)!, label: "target", parameterKey: "target" }] : [];
    case "race":
      return nodeRefArray(parameters.branches).map((target, parameterIndex) => ({ target, label: "race", parameterKey: "branches", parameterIndex }));
    default:
      return [];
  }
}

export function nodeRef(target: string): JsonRecord {
  return { "$node": target };
}

export function nodeRefId(value: unknown): string | null {
  return isRecord(value) && typeof value.$node === "string" && value.$node.length > 0 ? value.$node : null;
}

function nodeRefArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map(nodeRefId).filter((item): item is string => Boolean(item)) : [];
}

export function valueRef(source: "input" | "prev" | "workflow", path: Array<string | number>): JsonRecord {
  return { "$ref": { [source]: path } };
}

export function validateWorkflowReferenceSyntax(definition: JsonRecord): string[] {
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const nodeIds = new Set(nodes.map((node: JsonRecord) => String(node.id)).filter(Boolean));
  const errors: string[] = [];
  for (const node of nodes) {
    const nodeId = String(node.id ?? "<missing>");
    validateNodeRefs(node, nodeIds, nodeId, errors);
    validateExpressions(node.parameters, nodeIds, `${nodeId}.parameters`, errors);
    validateExpressions(node.wait, nodeIds, `${nodeId}.wait`, errors);
    validateExpressions(node.condition, nodeIds, `${nodeId}.condition`, errors);
    if (Array.isArray(node.transitions?.branches)) {
      for (const [index, branch] of node.transitions.branches.entries()) {
        validateExpressions(branch.when, nodeIds, `${nodeId}.transitions.branches[${index}].when`, errors);
      }
    }
  }
  return errors;
}

function validateNodeRefs(node: JsonRecord, nodeIds: Set<string>, nodeId: string, errors: string[]) {
  const transitions = isRecord(node.transitions) ? node.transitions : {};
  for (const key of directTransitionKeys) validateNodeRef(transitions[key], nodeIds, `${nodeId}.transitions.${key}`, errors, false);
  if (Array.isArray(transitions.branches)) {
    transitions.branches.forEach((branch: JsonRecord, index: number) => validateNodeRef(branch?.target, nodeIds, `${nodeId}.transitions.branches[${index}].target`, errors, true));
  }
  for (const { target, label } of controlFlowTargetValues(node)) {
    if (!nodeIds.has(target)) errors.push(`${nodeId}.${label} references missing node ${target}`);
  }
}

function validateNodeRef(value: unknown, nodeIds: Set<string>, label: string, errors: string[], required: boolean) {
  if (value == null && !required) return;
  const target = nodeRefId(value);
  if (!target) return errors.push(`${label} must be { "$node": "node_id" }`);
  if (!nodeIds.has(target)) errors.push(`${label} references missing node ${target}`);
}

function validateExpressions(value: unknown, nodeIds: Set<string>, label: string, errors: string[]) {
  if (value == null) return;
  if (typeof value === "string") {
    if (value.includes("{{") || value.includes("}}")) errors.push(`${label} uses removed template reference syntax`);
    return;
  }
  if (Array.isArray(value)) return value.forEach((item, index) => validateExpressions(item, nodeIds, `${label}[${index}]`, errors));
  if (!isRecord(value)) return;
  if ("$value" in value) errors.push(`${label} uses removed $value reference syntax`);
  const operators = ["$ref", "$concat", "$literal", "$node"].filter((key) => key in value);
  if (operators.length > 0 && Object.keys(value).length !== 1) errors.push(`${label} expression object must contain exactly one operator`);
  if (isRecord(value.$ref)) {
    if (typeof value.$ref.node === "string" && !nodeIds.has(value.$ref.node)) errors.push(`${label} references missing node ${value.$ref.node}`);
    for (const path of [value.$ref.input, value.$ref.prev, value.$ref.workflow, value.$ref.output]) {
      if (path !== undefined && !validRefPath(path)) errors.push(`${label} has invalid reference path`);
    }
  }
  if (Array.isArray(value.$concat)) value.$concat.forEach((item, index) => validateExpressions(item, nodeIds, `${label}.$concat[${index}]`, errors));
  if (operators.length === 0) Object.entries(value).forEach(([key, nested]) => validateExpressions(nested, nodeIds, `${label}.${key}`, errors));
}

function validRefPath(value: unknown): boolean {
  return Array.isArray(value) && value.every((item) => typeof item === "string" || (Number.isInteger(item) && Number(item) >= 0));
}

function workflowNodeKind(value: unknown): WorkflowNodeKind {
  return typeof value === "string" && ["start", ...workflowNodeKinds, "loop", "end"].includes(value) ? (value as WorkflowNodeKind) : "task";
}

function nodeSummary(node: JsonRecord, task?: ScheduledTask): string {
  switch (workflowNodeKind(node.kind)) {
    case "task":
      return task ? `${task.name} · ${task.action_name}.${task.action_function}` : `Task ${node.task_id ?? "-"}`;
    case "approval":
      return String(node.parameters?.prompt ?? "Approval required");
    case "condition": {
      const count = Array.isArray(node.transitions?.branches) ? node.transitions.branches.length : 0;
      return `${count} branch${count === 1 ? "" : "es"}`;
    }
    case "wait":
      return node.wait?.seconds ? `${node.wait.seconds}s` : "wait";
    case "subflow":
      return `workflow ${node.subflow_id ?? "-"}`;
    default:
      return workflowNodeKind(node.kind);
  }
}

function approvalPrompt(node: JsonRecord, state?: JsonRecord): string | undefined {
  if (workflowNodeKind(node.kind) !== "approval") return undefined;
  return String(state?.prompt ?? state?.approval?.prompt ?? node.parameters?.prompt ?? "Approval required");
}

function firstAvailableTransition(node: JsonRecord): WorkflowDirectTransitionKey {
  const transitions = isRecord(node.transitions) ? node.transitions : {};
  return directTransitionKeys.find((key) => !transitions[key]) ?? "next";
}
