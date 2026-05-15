import { MarkerType, type Edge, type Node } from "@vue-flow/core";
import type {
  JsonRecord,
  WorkflowDefinition,
  WorkflowConnectionHandle,
  WorkflowDirectTransitionKey,
  WorkflowEditorEdgeData,
  WorkflowEditorNodeRecord,
  WorkflowLayoutDirection,
  WorkflowLayoutPosition,
  WorkflowNodeKind,
  WorkflowRunDetail,
  ScheduledTask,
  RunSummary
} from "../types/models";
import { statusClassForNode } from "./status";

export const workflowNodeKinds: WorkflowNodeKind[] = [
  "action",
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
export const workflowConnectionHandles: WorkflowConnectionHandle[] = ["top", "right", "bottom", "left"];

export function buildGraphNodes(workflow: WorkflowDefinition, detail: WorkflowRunDetail | null, tasks: ScheduledTask[] = []): Node[] {
  const definition = workflow.definition ?? {};
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const layout = workflowLayoutNodes(definition);
  const fallbackLayout = autoArrangeWorkflowLayout(definition);
  const runByNode = new Map((detail?.nodes ?? []).map((run) => [run.node_id, run]));
  const taskById = new Map(tasks.filter((task) => task.id !== null).map((task) => [task.id, task]));
  return nodes.map((node: JsonRecord, index: number) => {
    const id = String(node.id ?? `step_${index + 1}`);
    const position = layout[id] ?? fallbackLayout[id] ?? { x: (index % 4) * 220, y: Math.floor(index / 4) * 90 };
    const run = runByNode.get(id);
    const status = run?.status ?? inferredNodeStatus(node, id, detail);
    const kind = workflowNodeKind(node.kind);
    return {
      id,
      type: "workflow",
      position: { x: Number(position.x ?? 0), y: Number(position.y ?? 0) },
      data: {
        title: id,
        kind,
        summary: nodeSummary(node, taskById.get(Number(node.task_id)) ?? null),
        statusLabel: run ? `${run.status} a${run.attempt}` : status,
        approvalPrompt: approvalPrompt(node, run?.state),
        running: status === "running" || status === "queued",
        status,
        protected: kind === "start" || kind === "end" || kind === "fail"
      },
      class: statusClassForNode(status)
    };
  });
}

export function workflowRunSearchText(run: RunSummary, workflowName = ""): string {
  return [
    run.id,
    run.workflow_id ?? "",
    workflowName,
    run.status,
    run.trigger ?? ""
  ].join(" ").toLowerCase();
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
        const handles = edgeHandles(definition, source, key);
        edges.push(graphEdge(source, target, key, { kind: "direct", transitionKey: key, ...handles, editable: true }));
      }
    }
    for (const [index, branch] of (transitions.branches ?? []).entries()) {
      const target = nodeRefId(branch.target);
      if (target && nodeIds.has(target)) {
        const handles = edgeHandles(definition, source, `branches.${index}`);
        edges.push(graphEdge(source, target, branch.label ?? `branch ${index + 1}`, { kind: "branch", branchIndex: index, ...handles, editable: true }));
      }
    }
    edges.push(...controlFlowEdges(definition, node, nodeIds));
  }
  return separateParallelEdges(edges);
}

export function autoArrangeWorkflowLayout(definition: JsonRecord, direction: WorkflowLayoutDirection = "horizontal"): Record<string, WorkflowLayoutPosition> {
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const ids = nodes.map((node: JsonRecord, index: number) => String(node.id ?? `step_${index + 1}`)).filter(Boolean);
  if (ids.length === 0) return {};

  const nodeIds = new Set(ids);
  const indexById = new Map(ids.map((id, index) => [id, index]));
  const edges = workflowLayoutEdges(nodes, nodeIds);
  const components = stronglyConnectedComponents(ids, edges);
  const componentById = new Map<string, number>();
  components.forEach((component, componentIndex) => {
    for (const id of component) componentById.set(id, componentIndex);
  });

  const componentEdges = new Map<number, Set<number>>();
  const incomingCounts = new Map<number, number>();
  components.forEach((_, index) => {
    componentEdges.set(index, new Set());
    incomingCounts.set(index, 0);
  });

  for (const edge of edges) {
    const sourceComponent = componentById.get(edge.source);
    const targetComponent = componentById.get(edge.target);
    if (sourceComponent === undefined || targetComponent === undefined || sourceComponent === targetComponent) continue;
    const targets = componentEdges.get(sourceComponent)!;
    if (targets.has(targetComponent)) continue;
    targets.add(targetComponent);
    incomingCounts.set(targetComponent, (incomingCounts.get(targetComponent) ?? 0) + 1);
  }

  const levels = componentLevels(components, componentEdges, incomingCounts, definition.start, indexById);
  const maxLevel = Math.max(0, ...levels);
  const grouped = Array.from({ length: maxLevel + 1 }, () => [] as number[]);
  levels.forEach((level, componentIndex) => grouped[level].push(componentIndex));

  for (const group of grouped) {
    group.sort((left, right) => componentSortKey(components[left], indexById) - componentSortKey(components[right], indexById));
  }

  const columnGap = 270;
  const rowGap = 150;
  const levelSlots = grouped.map((group) => group.reduce((total, componentIndex) => total + components[componentIndex].length, 0));
  const maxSlots = Math.max(1, ...levelSlots);
  const positions: Record<string, WorkflowLayoutPosition> = {};

  grouped.forEach((group, level) => {
    let slot = 0;
    const yOffset = ((maxSlots - levelSlots[level]) * rowGap) / 2;
    for (const componentIndex of group) {
      const component = [...components[componentIndex]].sort((left, right) => (indexById.get(left) ?? 0) - (indexById.get(right) ?? 0));
      for (const id of component) {
        const primary = level * columnGap;
        const secondary = yOffset + slot * rowGap;
        positions[id] = direction === "vertical" ? { x: secondary, y: primary } : { x: primary, y: secondary };
        slot += 1;
      }
    }
  });

  return positions;
}

export function autoArrangeWorkflowEdgeHandles(definition: JsonRecord, positions: Record<string, WorkflowLayoutPosition>) {
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  const nodeIds = new Set(nodes.map((node: JsonRecord) => String(node.id)).filter(Boolean));
  const setHandles = (source: string, semanticKey: string, target: string | null) => {
    if (!target || !nodeIds.has(source) || !nodeIds.has(target)) return;
    const handles = connectionHandlesForPositions(positions[source], positions[target]);
    setWorkflowEdgeHandles(definition, source, semanticKey, handles.sourceHandle, handles.targetHandle);
  };

  for (const node of nodes) {
    const source = String(node.id ?? "");
    const transitions = isRecord(node.transitions) ? node.transitions : {};
    for (const key of directTransitionKeys) setHandles(source, key, nodeRefId(transitions[key]));
    if (Array.isArray(transitions.branches)) {
      transitions.branches.forEach((branch: JsonRecord, index: number) => setHandles(source, `branches.${index}`, nodeRefId(branch?.target)));
    }
    for (const { target, parameterKey, parameterIndex } of controlFlowTargetValues(node)) {
      setHandles(source, parameterSemanticKey(parameterKey, parameterIndex), target);
    }
  }
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
    case "action":
      node.action = { provider: "", function: "", timeout_seconds: 300, configuration: {} };
      node.retry = { max_attempts: 1 };
      break;
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

export function createWorkflowTaskDraft(nodeId: string, taskId: number | null): ScheduledTask {
  return {
    id: taskId,
    name: `${titleCase(nodeId)} Task`,
    cron_schedule: "",
    action_name: "",
    action_function: "",
    enabled: false,
    timeout: 300,
    configuration: {
      task_type: "workflow",
      workflow_node_id: nodeId
    }
  };
}

export function copyWorkflowTaskDraft(task: ScheduledTask, nodeId: string, taskId: number | null): ScheduledTask {
  return stampWorkflowTaskConfiguration(
    {
      ...task,
      id: taskId,
      name: `${task.name || titleCase(nodeId)} copy`,
      configuration: { ...(task.configuration ?? {}) }
    },
    nodeId,
    null
  );
}

export function stampWorkflowTaskConfiguration(task: ScheduledTask, nodeId: string, workflowId: number | null): ScheduledTask {
  const configuration: JsonRecord = {
    ...(task.configuration ?? {}),
    task_type: "workflow",
    workflow_node_id: nodeId
  };
  if (workflowId !== null) configuration.workflow_id = workflowId;
  return {
    ...task,
    configuration
  };
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

export function isSameConnectionPointLoop(connection: {
  source?: string | null;
  target?: string | null;
  sourceHandle?: string | null;
  targetHandle?: string | null;
}): boolean {
  return Boolean(
    connection.source &&
      connection.target &&
      connection.source === connection.target &&
      connection.sourceHandle &&
      connection.targetHandle &&
      connection.sourceHandle === connection.targetHandle
  );
}

export function setWorkflowEdgeHandles(
  definition: JsonRecord,
  source: string,
  semanticKey: string,
  sourceHandle?: string | null,
  targetHandle?: string | null
) {
  definition.ui = isRecord(definition.ui) ? definition.ui : {};
  definition.ui.edge_handles = isRecord(definition.ui.edge_handles) ? definition.ui.edge_handles : {};
  definition.ui.edge_handles[edgeHandleKey(source, semanticKey)] = {
    sourceHandle: normalizeConnectionHandle(sourceHandle),
    targetHandle: normalizeConnectionHandle(targetHandle)
  };
}

export function removeWorkflowEdgeHandles(definition: JsonRecord, source: string, semanticKey: string) {
  const handles = definition.ui?.edge_handles;
  if (!isRecord(handles)) return;
  delete handles[edgeHandleKey(source, semanticKey)];
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

export function removeWorkflowEdge(node: JsonRecord, edge: Edge): boolean {
  if (removeEditableEdge(node, edge)) return true;
  const data = edge.data as WorkflowEditorEdgeData | undefined;
  if (data?.kind !== "control" || !data.parameterKey) return false;
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  if (typeof data.parameterIndex === "number" && Array.isArray(parameters[data.parameterKey])) {
    const current = parameters[data.parameterKey][data.parameterIndex];
    if (nodeRefId(current) !== edge.target && nodeRefId(current?.target) !== edge.target) return false;
    parameters[data.parameterKey].splice(data.parameterIndex, 1);
    return true;
  }
  if (nodeRefId(parameters[data.parameterKey]) === edge.target) {
    delete parameters[data.parameterKey];
    return true;
  }
  return false;
}

export function removeWorkflowNodeReferences(definition: JsonRecord, nodeId: string) {
  const nodes = Array.isArray(definition.nodes) ? definition.nodes : [];
  for (const node of nodes) {
    const transitions = isRecord(node.transitions) ? node.transitions : {};
    for (const key of directTransitionKeys) {
      if (nodeRefId(transitions[key]) === nodeId) delete transitions[key];
    }
    if (Array.isArray(transitions.branches)) {
      transitions.branches = transitions.branches.filter((branch: JsonRecord) => nodeRefId(branch?.target) !== nodeId);
    }

    const parameters = isRecord(node.parameters) ? node.parameters : {};
    for (const key of ["default", "body", "catch", "finally", "target"]) {
      if (nodeRefId(parameters[key]) === nodeId) delete parameters[key];
    }
    for (const key of ["branches", "wait_for", "cases"]) {
      if (!Array.isArray(parameters[key])) continue;
      parameters[key] = parameters[key].filter((item: unknown) => nodeRefId(item) !== nodeId && nodeRefId((item as JsonRecord)?.target) !== nodeId);
    }
  }
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
  ensureFailNode(nodes, ids);
  const previousStart =
    typeof nextDefinition.start === "string" && ids.has(nextDefinition.start) && nodeKindById(nodes, nextDefinition.start) !== "start"
      ? nextDefinition.start
      : firstNodeId(nodes, (kind) => kind !== "start" && kind !== "end" && kind !== "fail") ?? endId;
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

function ensureFailNode(nodes: JsonRecord[], ids: Set<string>): string {
  const existing = firstNodeId(nodes, (kind) => kind === "fail");
  if (existing) return existing;
  const id = uniqueNodeId("fail", ids);
  nodes.push({ id, kind: "fail" });
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
    if (node.kind === "end" || node.kind === "fail" || hasSuccessTransition(node)) continue;
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
  if (detail.run.active_node_id === id && isWorkflowRunDisplayStatus(detail.run.status)) return detail.run.status;
  if (node.kind === "end" && detail.run.active_node_id === id && detail.run.status === "succeeded") return "succeeded";
  if (node.kind === "fail" && detail.run.active_node_id === id && detail.run.status === "failed") return "failed";
  if (node.kind === "start" && detail.nodes.length > 0) return "succeeded";
  return undefined;
}

function isWorkflowRunDisplayStatus(status: string | undefined): status is string {
  return ["queued", "running", "debug_paused", "waiting", "approval_required", "blocked", "succeeded", "failed", "timed_out", "canceled"].includes(status ?? "");
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
    type: "smoothstep",
    source,
    target,
    sourceHandle: data.sourceHandle,
    targetHandle: data.targetHandle,
    label,
    data,
    updatable: data.editable,
    interactionWidth: 24,
    markerEnd: MarkerType.ArrowClosed
  };
}

function workflowLayoutEdges(nodes: JsonRecord[], nodeIds: Set<string>): Array<{ source: string; target: string }> {
  const edges: Array<{ source: string; target: string }> = [];
  for (const node of nodes) {
    const source = String(node.id ?? "");
    if (!source || !nodeIds.has(source)) continue;
    const transitions = isRecord(node.transitions) ? node.transitions : {};
    for (const key of directTransitionKeys) pushLayoutEdge(edges, source, nodeRefId(transitions[key]), nodeIds);
    if (Array.isArray(transitions.branches)) {
      for (const branch of transitions.branches) pushLayoutEdge(edges, source, nodeRefId(branch?.target), nodeIds);
    }
    for (const edge of parameterLayoutEdges(node, source, nodeIds)) edges.push(edge);
  }
  return dedupeLayoutEdges(edges);
}

function parameterLayoutEdges(node: JsonRecord, source: string, nodeIds: Set<string>): Array<{ source: string; target: string }> {
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  const edges: Array<{ source: string; target: string }> = [];
  switch (node.kind) {
    case "switch": {
      const cases = Array.isArray(parameters.cases) ? parameters.cases : [];
      for (const item of cases.filter(isRecord)) pushLayoutEdge(edges, source, nodeRefId(item.target), nodeIds);
      pushLayoutEdge(edges, source, nodeRefId(parameters.default), nodeIds);
      return edges;
    }
    case "parallel":
    case "race":
      for (const target of nodeRefArray(parameters.branches)) pushLayoutEdge(edges, source, target, nodeIds);
      return edges;
    case "join":
      for (const dependency of nodeRefArray(parameters.wait_for)) pushLayoutEdge(edges, dependency, source, nodeIds);
      return edges;
    case "try":
      for (const key of ["body", "catch", "finally"]) pushLayoutEdge(edges, source, nodeRefId(parameters[key]), nodeIds);
      return edges;
    case "loop":
    case "map":
      pushLayoutEdge(edges, source, nodeRefId(parameters.target), nodeIds);
      return edges;
    default:
      return edges;
  }
}

function pushLayoutEdge(edges: Array<{ source: string; target: string }>, source: string, target: string | null, nodeIds: Set<string>) {
  if (!target || source === target || !nodeIds.has(source) || !nodeIds.has(target)) return;
  edges.push({ source, target });
}

function dedupeLayoutEdges(edges: Array<{ source: string; target: string }>): Array<{ source: string; target: string }> {
  const seen = new Set<string>();
  return edges.filter((edge) => {
    const key = `${edge.source}\u0000${edge.target}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function stronglyConnectedComponents(ids: string[], edges: Array<{ source: string; target: string }>): string[][] {
  const adjacency = new Map(ids.map((id) => [id, [] as string[]]));
  for (const edge of edges) adjacency.get(edge.source)?.push(edge.target);

  const components: string[][] = [];
  const indexById = new Map<string, number>();
  const lowLinkById = new Map<string, number>();
  const stack: string[] = [];
  const onStack = new Set<string>();
  let nextIndex = 0;

  function visit(id: string) {
    indexById.set(id, nextIndex);
    lowLinkById.set(id, nextIndex);
    nextIndex += 1;
    stack.push(id);
    onStack.add(id);

    for (const target of adjacency.get(id) ?? []) {
      if (!indexById.has(target)) {
        visit(target);
        lowLinkById.set(id, Math.min(lowLinkById.get(id)!, lowLinkById.get(target)!));
      } else if (onStack.has(target)) {
        lowLinkById.set(id, Math.min(lowLinkById.get(id)!, indexById.get(target)!));
      }
    }

    if (lowLinkById.get(id) !== indexById.get(id)) return;
    const component: string[] = [];
    for (;;) {
      const member = stack.pop();
      if (!member) break;
      onStack.delete(member);
      component.push(member);
      if (member === id) break;
    }
    components.push(component);
  }

  for (const id of ids) {
    if (!indexById.has(id)) visit(id);
  }

  return components;
}

function componentLevels(
  components: string[][],
  componentEdges: Map<number, Set<number>>,
  incomingCounts: Map<number, number>,
  start: unknown,
  indexById: Map<string, number>
): number[] {
  const levels = components.map(() => 0);
  const startComponent = typeof start === "string" ? components.findIndex((component) => component.includes(start)) : -1;
  const queue = [...components.keys()]
    .filter((componentIndex) => incomingCounts.get(componentIndex) === 0)
    .sort((left, right) => {
      if (left === startComponent) return -1;
      if (right === startComponent) return 1;
      return componentSortKey(components[left], indexById) - componentSortKey(components[right], indexById);
    });

  for (let index = 0; index < queue.length; index += 1) {
    const source = queue[index];
    for (const target of componentEdges.get(source) ?? []) {
      levels[target] = Math.max(levels[target], levels[source] + 1);
      incomingCounts.set(target, (incomingCounts.get(target) ?? 0) - 1);
      if (incomingCounts.get(target) === 0) queue.push(target);
    }
  }

  return levels;
}

function componentSortKey(component: string[], indexById: Map<string, number>): number {
  return Math.min(...component.map((id) => indexById.get(id) ?? 0));
}

function edgeId(source: string, target: string, label: string, data: WorkflowEditorEdgeData): string {
  return [source, data.kind, data.transitionKey ?? data.parameterKey ?? data.branchIndex ?? label, data.parameterIndex ?? "", data.sourceHandle ?? "", data.targetHandle ?? "", target]
    .map((part) => encodeURIComponent(String(part)))
    .join(":");
}

function separateParallelEdges(edges: Edge[]): Edge[] {
  const groups = new Map<string, Edge[]>();
  for (const edge of edges) {
    const key = [edge.source, edge.target, edge.sourceHandle ?? "", edge.targetHandle ?? ""].join("\u0000");
    const group = groups.get(key) ?? [];
    group.push(edge);
    groups.set(key, group);
  }

  return edges.map((edge) => {
    const group = groups.get([edge.source, edge.target, edge.sourceHandle ?? "", edge.targetHandle ?? ""].join("\u0000")) ?? [edge];
    if (group.length === 1) return edge;
    const index = group.findIndex((item) => item.id === edge.id);
    return {
      ...edge,
      pathOptions: { offset: 18 + index * 18, borderRadius: 8 },
      zIndex: index + 1
    };
  });
}

function controlFlowEdges(definition: JsonRecord, node: JsonRecord, nodeIds: Set<string>): Edge[] {
  const source = String(node.id ?? "");
  return controlFlowTargetValues(node)
    .filter(({ target }) => nodeIds.has(target))
    .map(({ target, label, parameterKey, parameterIndex }) =>
      graphEdge(source, target, label, {
        kind: "control",
        parameterKey,
        parameterIndex,
        ...edgeHandles(definition, source, parameterSemanticKey(parameterKey, parameterIndex)),
        editable: false
      })
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
  return typeof value === "string" && ["start", ...workflowNodeKinds, "loop", "end", "fail"].includes(value) ? (value as WorkflowNodeKind) : "task";
}

export function workflowNodeActionConfig(node: JsonRecord, task: ScheduledTask | null = null): { provider: string; action: string } {
  const action = isRecord(node.action) ? node.action : {};
  return {
    provider: String(action.provider ?? node.action_name ?? task?.action_name ?? ""),
    action: String(action.function ?? node.action_function ?? task?.action_function ?? "")
  };
}

function nodeSummary(node: JsonRecord, task: ScheduledTask | null = null): string {
  switch (workflowNodeKind(node.kind)) {
    case "action":
    case "task": {
      const config = workflowNodeActionConfig(node, task);
      const action = config.provider ? `${config.provider}.${config.action || ""}` : "unconfigured action";
      return `Action: ${action}`;
    }
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
    case "fail":
      return "Workflow failure";
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

function edgeHandles(definition: JsonRecord, source: string, semanticKey: string): Pick<WorkflowEditorEdgeData, "sourceHandle" | "targetHandle"> {
  const saved = definition.ui?.edge_handles?.[edgeHandleKey(source, semanticKey)];
  return {
    sourceHandle: normalizeConnectionHandle(saved?.sourceHandle) ?? "bottom",
    targetHandle: normalizeConnectionHandle(saved?.targetHandle) ?? "top"
  };
}

function connectionHandlesForPositions(source?: WorkflowLayoutPosition, target?: WorkflowLayoutPosition): Pick<WorkflowEditorEdgeData, "sourceHandle" | "targetHandle"> {
  if (!source || !target) return { sourceHandle: "bottom", targetHandle: "top" };
  const dx = target.x - source.x;
  const dy = target.y - source.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx >= 0 ? { sourceHandle: "right", targetHandle: "left" } : { sourceHandle: "left", targetHandle: "right" };
  }
  return dy >= 0 ? { sourceHandle: "bottom", targetHandle: "top" } : { sourceHandle: "top", targetHandle: "bottom" };
}

export function parameterSemanticKey(parameterKey?: string, parameterIndex?: number): string {
  return typeof parameterIndex === "number" ? `${parameterKey}.${parameterIndex}` : String(parameterKey ?? "control");
}

function edgeHandleKey(source: string, semanticKey: string): string {
  return `${source}:${semanticKey}`;
}

function titleCase(value: string): string {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function normalizeConnectionHandle(value: unknown): WorkflowConnectionHandle | undefined {
  return workflowConnectionHandles.includes(value as WorkflowConnectionHandle) ? (value as WorkflowConnectionHandle) : undefined;
}

function titleFromNodeId(nodeId: string): string {
  return nodeId
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase())
    .trim() || "Workflow";
}
