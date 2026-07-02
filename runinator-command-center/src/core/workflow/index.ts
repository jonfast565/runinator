import type {
  JsonRecord,
  WorkflowDefinition,
  WorkflowConnectionHandle,
  WorkflowDirectTransitionKey,
  WorkflowEdgeEditorDraft,
  WorkflowEdgeEditorMatchKind,
  WorkflowEdgeLabelAnchor,
  WorkflowEdgeLabelOffset,
  WorkflowEdgeStyle,
  WorkflowEdgeSemanticOption,
  WorkflowEditorEdgeData,
  WorkflowInlineEditDescriptor,
  WorkflowEditorNodeRecord,
  WorkflowLayoutDirection,
  WorkflowLayoutPosition,
  WorkflowNodeKind,
  WorkflowSemanticHandle,
  WorkflowValidationIssue,
  WorkflowValidationSeverity,
  WorkflowRunDetail,
  RunSummary,
  ProviderMetadata,
  ActionResultMetadata,
} from "../domain/models";
import { asJsonRecord, asJsonValue } from "../domain/json";
import { isJsonRecord } from "../domain/json";
import { coerceDebugFrame } from "../domain/models/workflow-state";
import type { IconName } from "../domain/icons";
import type { GraphEdgeLike, GraphEdgeModel, GraphNodeModel } from "./graph-model";
import { statusClassForNode } from "../utils/status";
import { displayValue, isBlankValue } from "../utils/values";

export const workflowNodeKinds: WorkflowNodeKind[] = [
  "action",
  "approval",
  "gate",
  "signal",
  "loop",
  "condition",
  "wait",
  "switch",
  "toggle",
  "percentage",
  "parallel",
  "join",
  "try",
  "map",
  "race",
  "output",
  "input",
  "config",
  "subflow",
  "assert",
  "transform",
  "audit",
  "checkpoint",
  "mutex",
  "throttle",
  "await_run",
  "debounce",
  "collect",
  "barrier",
  "circuit_breaker",
  "event_source",
];

// icon name and a one-line description for every node kind, used by node chrome and the palette.
export interface WorkflowNodeKindInfo {
  icon: IconName;
  description: string;
}

export const workflowNodeKindInfo: Record<WorkflowNodeKind, WorkflowNodeKindInfo> = {
  start: { icon: "play", description: "Entry point where the workflow run begins." },
  action: { icon: "bolt", description: "Runs a task through a provider action." },
  wait: { icon: "clock", description: "Pauses the run for a fixed delay or until a time." },
  condition: { icon: "branch", description: "Routes down a branch based on a boolean expression." },
  switch: { icon: "switch", description: "Routes to one of several cases by matching a value." },
  toggle: {
    icon: "toggle",
    description: "A light switch: routes to on or off by a value's truthiness.",
  },
  percentage: {
    icon: "percentage",
    description: "Weighted rollout: routes to a bucket by a stable hash of a key.",
  },
  approval: { icon: "approve", description: "Halts until a human approves or rejects." },
  gate: {
    icon: "shield",
    description: "Blocks until an automated/policy check or manual gate opens.",
  },
  signal: {
    icon: "bell",
    description: "Pauses until a named external signal is delivered to the run.",
  },
  loop: { icon: "loop", description: "Repeats its target node while a condition holds." },
  parallel: { icon: "parallel", description: "Fans out into branches that run concurrently." },
  join: { icon: "join", description: "Waits for upstream branches to finish before continuing." },
  try: { icon: "shield", description: "Guards a body node and catches failures with a handler." },
  map: { icon: "grid", description: "Runs its target once for each item in a collection." },
  race: { icon: "race", description: "Runs branches concurrently; the first to finish wins." },
  output: { icon: "output", description: "Publishes output without interrupting the flow." },
  input: { icon: "message", description: "Waits for a user-supplied value from the UI." },
  subflow: { icon: "workflow", description: "Invokes another workflow as a nested step." },
  config: { icon: "gear", description: "Sets configuration values for downstream nodes." },
  end: { icon: "flag", description: "Terminal node that completes the run successfully." },
  fail: { icon: "alert", description: "Terminal node that ends the run as failed." },
  assert: {
    icon: "check",
    description: "Evaluates named boolean assertions; fails with a structured violation list.",
  },
  transform: {
    icon: "gear",
    description: "Resolves named expression bindings into the run context; no side effects.",
  },
  audit: {
    icon: "file",
    description: "Appends a tamper-evident audit record to the workflow log.",
  },
  checkpoint: {
    icon: "save",
    description:
      "Snapshots run state at a named point; enables rollback via the control-plane API.",
  },
  mutex: {
    icon: "lock",
    description: "Acquires a named distributed mutex; parks until the lock is available.",
  },
  throttle: {
    icon: "hourglass",
    description: "Enforces a cross-run rate limit; parks until a token is available.",
  },
  await_run: {
    icon: "runs",
    description: "Waits for one or more independently-started runs to reach a terminal state.",
  },
  debounce: {
    icon: "clock",
    description: "Parks with a trailing delay that resets on re-trigger; collapses event bursts.",
  },
  collect: {
    icon: "list",
    description: "Accumulates externally-delivered items until a count or time threshold is met.",
  },
  barrier: {
    icon: "join",
    description:
      "Parks until N runs reach this named barrier; the last arrival releases all waiters.",
  },
  circuit_breaker: {
    icon: "shield",
    description: "Tracks failure rates across runs; fast-fails or routes to fallback when tripped.",
  },
  event_source: {
    icon: "bell",
    description:
      "Subscribes to a named event stream; drives a body subgraph on each matching event.",
  },
};

export function workflowNodeKindIcon(kind: string): IconName {
  return workflowNodeKindInfo[kind as WorkflowNodeKind].icon;
}

// human-friendly label for a node kind: the wire value is snake_case (e.g. `await_run`,
// `circuit_breaker`), which reads poorly in the palette/chrome, so render it title-cased.
export function workflowNodeKindLabel(kind: string): string {
  return titleCase(kind);
}

export function workflowNodeKindDescription(kind: string): string {
  return workflowNodeKindInfo[kind as WorkflowNodeKind].description;
}

export const directTransitionKeys: WorkflowDirectTransitionKey[] = [
  "next",
  "on_success",
  "on_failure",
  "on_timeout",
  "on_reject",
];
export const workflowConnectionHandles: WorkflowConnectionHandle[] = [
  "top",
  "right",
  "bottom",
  "left",
];
export const workflowEdgeStyles: WorkflowEdgeStyle[] = ["bezier", "straight", "square"];
const semanticTargetHandleId = "target:in";

export function buildGraphNodeModels(
  workflow: WorkflowDefinition,
  detail: WorkflowRunDetail | null,
  subflowNames?: Map<string, string>,
  providers: ProviderMetadata[] = [],
): GraphNodeModel[] {
  const definition = workflow.definition;
  const nodes = recordArray(definition.nodes);
  const issuesByNode = validationIssuesByNode(validateWorkflowIssues(definition, providers));
  const layout = workflowLayoutNodes(definition);
  const fallbackLayout = autoArrangeWorkflowLayout(definition);
  const detailNodes = detail?.nodes ?? [];
  const runByNode = new Map(detailNodes.map((run) => [run.node_id, run]));
  const executionCounts = workflowRunExecutionCounts(detailNodes);
  const debug = coerceDebugFrame(detail?.run.state?.debug);
  const breakpointSet = new Set<string>(debug?.breakpoints ?? []);
  return nodes.map((node: JsonRecord, index: number) => {
    const id = displayValue(node.id) || `step_${String(index + 1)}`;
    const layoutPosition = layout[id] ?? fallbackLayout[id];
    const position = isRecord(layoutPosition)
      ? layoutPosition
      : { x: (index % 4) * 220, y: Math.floor(index / 4) * 90 };
    const run = runByNode.get(id);
    const status = run?.status ?? inferredNodeStatus(node, id, detail);
    const kind = workflowNodeKind(node.kind);
    return {
      id,
      type: "workflow",
      position: { x: Number(position.x ?? 0), y: Number(position.y ?? 0) },
      data: {
        title: nodeDisplayName(node, id),
        nodeId: id,
        kind,
        summary: nodeSummary(node, subflowNames),
        semanticHandles: workflowNodeSemanticHandles(node),
        inlineEdit: workflowInlineEditDescriptor(node),
        validationIssues: issuesByNode.get(id) ?? [],
        validationCount: issuesByNode.get(id)?.length ?? 0,
        validationSeverity: validationSeverity(issuesByNode.get(id) ?? []),
        statusLabel: run ? `${run.status} a${String(run.attempt)}` : status,
        executionCount: executionCounts.get(id) ?? 0,
        approvalPrompt: approvalPrompt(node, run?.state),
        inputPrompt: inputPrompt(node, run?.state),
        running: status === "running" || status === "queued",
        status,
        protected: kind === "start" || kind === "end" || kind === "fail",
        locked: kind === "start" || kind === "end" || kind === "fail" || node.locked === true,
        skipped: node.skipped === true,
        debugBreakpoint: breakpointSet.has(id),
      },
      class: statusClassForNode(status),
    };
  });
}

export function workflowRunSearchText(run: RunSummary, workflowName = ""): string {
  return [run.id, run.workflow_id ?? "", workflowName, run.status, run.trigger ?? ""]
    .join(" ")
    .toLowerCase();
}

function workflowRunExecutionCounts(nodes: WorkflowRunDetail["nodes"]): Map<string, number> {
  const counts = new Map<string, number>();

  for (const node of nodes) {
    const executions = workflowNodeRunExecutionCount(node);

    if (executions <= 0) {
      continue;
    }

    counts.set(node.node_id, (counts.get(node.node_id) ?? 0) + executions);
  }

  return counts;
}

function workflowNodeRunExecutionCount(node: WorkflowRunDetail["nodes"][number]): number {
  if (Number.isFinite(node.attempt) && node.attempt > 0) {
    return Math.floor(node.attempt);
  }

  return node.status === "queued" ? 0 : 1;
}

export function buildGraphEdgeModels(workflow: WorkflowDefinition): GraphEdgeModel[] {
  const definition = workflow.definition;
  const nodes = recordArray(definition.nodes);
  const nodeIds = new Set(nodes.map((node: JsonRecord) => String(node.id)));
  const issuesByEdge = validationIssuesByEdge(validateWorkflowIssues(definition));
  const edges: GraphEdgeModel[] = [];

  const edgeData = (
    source: string,
    semanticKey: string,
    data: WorkflowEditorEdgeData,
  ): WorkflowEditorEdgeData => {
    const issues = issuesByEdge.get(edgeValidationKey(source, semanticKey)) ?? [];
    return {
      ...data,
      validationCount: issues.length,
      validationSeverity: validationSeverity(issues),
      validationMessages: issues.map((issue) => issue.message),
    };
  };

  for (const node of nodes) {
    const source = displayValue(node.id);
    const transitions = asRecord(node.transitions);

    for (const key of directTransitionKeys) {
      const target = nodeRefId(transitions[key]);

      if (target && nodeIds.has(target)) {
        const handles = edgeHandles(definition, source, key);
        edges.push(
          graphEdge(
            source,
            target,
            key,
            edgeData(source, key, {
              kind: "direct",
              transitionKey: key,
              ...handles,
              editable: true,
            }),
          ),
        );
      }
    }

    for (const [index, entry] of asArray(transitions.branches).entries()) {
      const branch = asRecord(entry);
      const target = nodeRefId(branch.target);

      if (target && nodeIds.has(target)) {
        const semanticKey = `branches.${String(index)}`;
        const handles = edgeHandles(definition, source, semanticKey);
        const base = displayValue(branch.label) || `branch ${String(index + 1)}`;
        const label = typeof branch.priority === "number" ? `#${String(branch.priority)} ${base}` : base;
        edges.push(
          graphEdge(
            source,
            target,
            label,
            edgeData(source, semanticKey, {
              kind: "branch",
              branchIndex: index,
              ...handles,
              editable: true,
            }),
          ),
        );
      }
    }

    edges.push(...controlFlowEdges(definition, node, nodeIds, issuesByEdge));
  }

  return separateParallelEdges(edges);
}

// control-flow kinds carry their own parameter-based routes; condition has its own branch options;
// terminals and start have no user-defined predicate routes. everything else is a default-transition
// node that can host predicate edges.
const predicateEdgeExcludedKinds = new Set([
  "condition",
  "switch",
  "parallel",
  "race",
  "join",
  "try",
  "loop",
  "map",
  "start",
  "end",
  "fail",
]);

function supportsPredicateEdges(kind: string): boolean {
  return !predicateEdgeExcludedKinds.has(kind);
}

export function workflowEdgeSemanticOptions(node: JsonRecord): WorkflowEdgeSemanticOption[] {
  const options: WorkflowEdgeSemanticOption[] = directTransitionKeys.map((key) => ({
    id: `direct:${key}`,
    label: transitionLabel(key),
    description: `Set ${key} transition`,
  }));
  const kind = workflowNodeKind(node.kind);
  const transitions = isRecord(node.transitions) ? node.transitions : {};

  if (kind === "condition") {
    const branches = asArray(transitions.branches);
    branches.forEach((_, index) => {
      options.push({
        id: `branch:${String(index)}`,
        label: `Condition branch ${String(index + 1)}`,
        description: "Update an existing condition branch",
      });
    });
    options.push({
      id: "branch:new",
      label: "New condition branch",
      description: "Add a conditional route",
    });
  } else if (supportsPredicateEdges(kind)) {
    // predicate edges attach a user-defined when -> target route to any default-transition node,
    // evaluated before status routing in ascending priority order.
    const branches = asArray(transitions.branches);
    branches.forEach((_, index) => {
      options.push({
        id: `branch:${String(index)}`,
        label: `Predicate edge ${String(index + 1)}`,
        description: "Update a conditional route",
      });
    });
    options.push({
      id: "branch:new",
      label: "New predicate edge",
      description: "Add a conditional route evaluated by priority",
    });
  }

  const parameters = isRecord(node.parameters) ? node.parameters : {};

  if (kind === "switch") {
    const cases = recordArray(parameters.cases);
    cases.forEach((_, index) => {
      options.push({
        id: `control:cases:${String(index)}`,
        label: `Switch case ${String(index + 1)}`,
        description: "Update an existing switch case",
      });
    });
    options.push({
      id: "control:cases:new",
      label: "New switch case",
      description: "Add a switch case route",
    });
    options.push({
      id: "control:default",
      label: "Switch default",
      description: "Set the default switch route",
    });
  }

  if (kind === "toggle") {
    options.push({
      id: "control:on",
      label: "Toggle on",
      description: "Node routed to when the value is truthy",
    });
    options.push({
      id: "control:off",
      label: "Toggle off",
      description: "Node routed to when the value is falsy",
    });
  }

  if (kind === "percentage") {
    const buckets = asArray(parameters.buckets);
    buckets.forEach((_, index) => {
      options.push({
        id: `control:buckets:${String(index)}`,
        label: `Bucket ${String(index + 1)}`,
        description: "Update an existing percentage bucket target",
      });
    });
    options.push({
      id: "control:default",
      label: "Percentage default",
      description: "Fallback route when no bucket matches",
    });
  }

  if (kind === "parallel" || kind === "race") {
    const branches = asArray(parameters.branches);
    branches.forEach((_, index) => {
      options.push({
        id: `control:branches:${String(index)}`,
        label: `${titleCase(kind)} branch ${String(index + 1)}`,
        description: "Update an existing branch target",
      });
    });
    options.push({
      id: "control:branches:new",
      label: `New ${kind} branch`,
      description: "Add a branch target",
    });
  }

  if (kind === "join") {
    const dependencies = asArray(parameters.wait_for);
    dependencies.forEach((_, index) => {
      options.push({
        id: `control:wait_for:${String(index)}`,
        label: `Join dependency ${String(index + 1)}`,
        description: "Update an existing join dependency",
      });
    });
    options.push({
      id: "control:wait_for:new",
      label: "New join dependency",
      description: "Add a node this join waits for",
    });
  }

  if (kind === "try") {
    options.push(
      { id: "control:body", label: "Try body", description: "Set the guarded body node" },
      { id: "control:catch", label: "Try catch", description: "Set the error handler node" },
      { id: "control:finally", label: "Try finally", description: "Set the cleanup node" },
    );
  }

  if (kind === "loop" || kind === "map") {
    options.push({
      id: "control:target",
      label: `${titleCase(kind)} target`,
      description: "Set the repeated target node",
    });
  }

  return options;
}

export function workflowNodeSemanticHandles(node: JsonRecord): WorkflowSemanticHandle[] {
  const handles: WorkflowSemanticHandle[] = [
    { id: semanticTargetHandleId, label: "in", type: "target" },
  ];

  for (const option of workflowEdgeSemanticOptions(node)) {
    handles.push({
      id: semanticSourceHandleId(option.id),
      label: option.label,
      type: "source",
      semanticOptionId: option.id,
    });
  }

  return handles;
}

// every node exposes a free-text display name; actions/configuration are edited in the modal instead.
export function workflowInlineEditDescriptor(
  node: JsonRecord,
): WorkflowInlineEditDescriptor | null {
  return { label: "Name", value: displayValue(node.name), valueKind: "text" };
}

export function applyWorkflowInlineNodeEdit(
  definition: JsonRecord,
  nodeId: string,
  nextId: string,
  inlineValue: string,
): { ok: true; nodeId: string } | { ok: false; message: string } {
  const nodes = recordArray(definition.nodes);
  const node = nodes.find((item: JsonRecord) => String(item.id) === nodeId);

  if (!node) {
    return { ok: false, message: "Node no longer exists" };
  }

  const trimmedId = nextId.trim();

  if (!trimmedId) {
    return { ok: false, message: "Node ID is required" };
  }

  if (trimmedId !== nodeId && nodes.some((item: JsonRecord) => String(item.id) === trimmedId)) {
    return { ok: false, message: `Node ID ${trimmedId} already exists` };
  }

  if (trimmedId !== nodeId) {
    renameWorkflowNodeReferences(definition, nodeId, trimmedId);
  }

  node.id = trimmedId;

  // the inline editor only manages the display name; node activity is edited in the step modal.
  const name = inlineValue.trim();

  if (name) {
    node.name = name;
  } else {
    delete node.name;
  }

  return { ok: true, nodeId: trimmedId };
}

export function renameWorkflowNodeReferences(
  definition: JsonRecord,
  previousId: string,
  nextId: string,
) {
  if (!previousId || !nextId || previousId === nextId) {
    return;
  }

  const nodes = recordArray(definition.nodes);

  if (definition.start === previousId) {
    definition.start = nextId;
  }

  for (const node of nodes) {
    renameNodeRefs(node.transitions, previousId, nextId);
    renameNodeRefs(node.parameters, previousId, nextId);
    renameNodeRefs(node.wait, previousId, nextId);
    renameNodeRefs(node.condition, previousId, nextId);
  }

  renameWorkflowEdgeHandleSource(definition, previousId, nextId);
}

export function validateWorkflowIssues(
  definition: JsonRecord,
  providers: ProviderMetadata[] = [],
): WorkflowValidationIssue[] {
  const nodes = recordArray(definition.nodes);
  const issues: WorkflowValidationIssue[] = [];
  const ids = new Map<string, number>();

  for (const node of nodes) {
    const nodeId = displayValue(node.id);

    if (!nodeId) {
      issues.push({ severity: "error", nodeId: "<missing>", message: "Node ID is required" });
      continue;
    }

    ids.set(nodeId, (ids.get(nodeId) ?? 0) + 1);
  }

  for (const [id, count] of ids.entries()) {
    if (count > 1) {
      issues.push({ severity: "error", nodeId: id, message: `Duplicate node ID ${id}` });
    }
  }

  const nodeIds = new Set(ids.keys());

  const start = typeof definition.start === "string" ? definition.start : "";

  if (start && !nodeIds.has(start)) {
    issues.push({
      severity: "error",
      nodeId: start,
      message: `Workflow start references missing node ${start}`,
    });
  }

  for (const node of nodes) {
    const nodeId = displayValue(node.id) || "<missing>";
    const transitions = isRecord(node.transitions) ? node.transitions : {};

    for (const key of directTransitionKeys) {
      pushNodeRefIssue(issues, nodeIds, nodeId, key, transitions[key], false);
    }

    recordArray(transitions.branches).forEach((branch, index) => {
      pushNodeRefIssue(issues, nodeIds, nodeId, `branches.${String(index)}`, branch.target, true);
    });

    pushControlFlowIssues(issues, node, nodeIds, nodeId);
    pushConnectivityIssues(issues, node, nodeId);
    pushExpressionIssues(issues, node.parameters, nodeIds, nodeId, `${nodeId}.parameters`);
    pushExpressionIssues(issues, node.wait, nodeIds, nodeId, `${nodeId}.wait`);
    pushExpressionIssues(issues, node.condition, nodeIds, nodeId, `${nodeId}.condition`);

    recordArray(transitions.branches).forEach((branch, index) => {
      pushExpressionIssues(
        issues,
        branch.when,
        nodeIds,
        nodeId,
        `${nodeId}.transitions.branches[${String(index)}].when`,
        `branches.${String(index)}`,
      );
    });

    pushProviderIssues(issues, node, providers, nodeId);
  }

  return issues;
}

function pushConnectivityIssues(
  issues: WorkflowValidationIssue[],
  node: JsonRecord,
  nodeId: string,
) {
  const kind = workflowNodeKind(node.kind);

  if (kind === "end" || kind === "fail") {
    return;
  }

  if (hasSuccessTransition(node)) {
    return;
  }

  issues.push({ severity: "error", nodeId, message: `${nodeId} has no outgoing path` });
}

export function workflowEdgeOptionId(edge: GraphEdgeLike): string {
  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (data?.kind === "direct" && data.transitionKey) {
    return `direct:${data.transitionKey}`;
  }

  if (data?.kind === "branch" && typeof data.branchIndex === "number") {
    return `branch:${String(data.branchIndex)}`;
  }

  if (data?.kind === "control" && data.parameterKey) {
    return typeof data.parameterIndex === "number"
      ? `control:${data.parameterKey}:${String(data.parameterIndex)}`
      : `control:${data.parameterKey}`;
  }

  return "";
}

export function workflowEdgeEditorDraft(
  workflow: WorkflowDefinition,
  edge: GraphEdgeLike,
): WorkflowEdgeEditorDraft | null {
  const definition = workflow.definition;
  const nodes = recordArray(definition.nodes);
  const node = nodes.find((item: JsonRecord) => String(item.id) === edge.source);

  if (!node) {
    return null;
  }

  const optionId = workflowEdgeOptionId(edge);

  if (!optionId) {
    return null;
  }

  const base = defaultWorkflowEdgeEditorDraft(edge, optionId);
  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (data?.kind === "branch" && typeof data.branchIndex === "number") {
    const branches = asArray(asRecord(node.transitions).branches);
    const branch = asRecord(branches[data.branchIndex]);
    return {
      ...base,
      label: displayValue(branch.label),
      whenJson: stringifyJson(branch.when ?? defaultConditionBranchWhen()),
      canEditLabel: true,
      canEditCondition: true,
      canMove: true,
      orderIndex: data.branchIndex,
      orderCount: branches.length,
      priority: typeof branch.priority === "number" ? branch.priority : null,
      canEditPriority: true,
    };
  }

  if (
    data?.kind === "control" &&
    data.parameterKey === "cases" &&
    typeof data.parameterIndex === "number"
  ) {
    const cases = asArray(asRecord(node.parameters).cases);
    const switchCase = asRecord(cases[data.parameterIndex]);
    const match = switchCaseMatchDraft(switchCase);
    return {
      ...base,
      label: displayValue(switchCase.label),
      matchKind: match.kind,
      matchJson: stringifyJson(match.value),
      canEditLabel: true,
      canEditSwitchCase: true,
      canMove: true,
      orderIndex: data.parameterIndex,
      orderCount: cases.length,
    };
  }

  if (data?.kind === "control" && data.parameterKey && typeof data.parameterIndex === "number") {
    const values = asArray(asRecord(node.parameters)[data.parameterKey]);
    return {
      ...base,
      canMove: ["branches", "wait_for"].includes(data.parameterKey),
      orderIndex: data.parameterIndex,
      orderCount: values.length,
    };
  }

  return base;
}

export function applyWorkflowEdgeEditorDraft(
  definition: JsonRecord,
  previousEdge: GraphEdgeLike | null,
  draft: WorkflowEdgeEditorDraft,
): { ok: true; semanticKey: string } | { ok: false; message: string } {
  const parsed = parseWorkflowEdgeDraftValues(draft);

  if (!parsed.ok) {
    return parsed;
  }

  const nodes = recordArray(definition.nodes);
  const sourceNode = nodes.find((node: JsonRecord) => String(node.id) === draft.source);

  if (!sourceNode) {
    return { ok: false, message: "Edge source node no longer exists" };
  }

  if (!draft.target) {
    return { ok: false, message: "Edge target is required" };
  }

  const previousOptionId = previousEdge ? workflowEdgeOptionId(previousEdge) : "";

  if (
    previousEdge &&
    (previousEdge.source !== draft.source || previousOptionId !== draft.optionId)
  ) {
    const previousSourceNode = nodes.find(
      (node: JsonRecord) => String(node.id) === previousEdge.source,
    );

    if (previousSourceNode) {
      removeWorkflowEdge(previousSourceNode, previousEdge);
    }

    removeEdgeHandlesForEdge(definition, previousEdge);
  }

  const semanticKey = writeWorkflowEdgeDraft(sourceNode, draft, parsed);

  if (!semanticKey) {
    return { ok: false, message: "Choose a valid edge type" };
  }

  setWorkflowEdgeHandles(
    definition,
    draft.source,
    semanticKey,
    draft.sourceHandle,
    draft.targetHandle,
    draft.edgeStyle,
    undefined,
    { position: draft.labelAnchor / 100 },
  );
  return { ok: true, semanticKey };
}

export function moveWorkflowEdgeEditorDraft(
  definition: JsonRecord,
  draft: WorkflowEdgeEditorDraft,
  direction: -1 | 1,
): { ok: true; draft: WorkflowEdgeEditorDraft } | { ok: false; message: string } {
  const location = orderedEdgeLocation(definition, draft);

  if (!location) {
    return { ok: false, message: "This edge cannot be reordered" };
  }

  const nextIndex = location.index + direction;

  if (nextIndex < 0 || nextIndex >= location.items.length) {
    return { ok: false, message: "Edge is already at that boundary" };
  }

  [location.items[location.index], location.items[nextIndex]] = [
    location.items[nextIndex],
    location.items[location.index],
  ];
  swapWorkflowEdgeHandles(
    definition,
    draft.source,
    location.semanticKey(location.index),
    location.semanticKey(nextIndex),
  );
  return {
    ok: true,
    draft: {
      ...draft,
      optionId: location.optionId(nextIndex),
      edgeId: "",
      orderIndex: nextIndex,
      orderCount: location.items.length,
    },
  };
}

function defaultWorkflowEdgeEditorDraft(edge: GraphEdgeLike, optionId: string): WorkflowEdgeEditorDraft {
  const data = edge.data as WorkflowEditorEdgeData | undefined;
  return {
    edgeId: edge.id,
    source: edge.source,
    target: edge.target,
    optionId,
    sourceHandle: edge.sourceHandle,
    targetHandle: edge.targetHandle,
    edgeStyle: normalizeWorkflowEdgeStyle(data?.edgeStyle),
    labelAnchor: Math.round((normalizeLabelAnchor(data?.labelAnchor)?.position ?? 0.5) * 100),
    label: "",
    whenJson: stringifyJson(defaultConditionBranchWhen()),
    matchKind: "equals",
    matchJson: stringifyJson(true),
    canEditLabel: false,
    canEditCondition: false,
    canEditSwitchCase: false,
    canMove: false,
    orderIndex: -1,
    orderCount: 0,
    priority: null,
    canEditPriority: false,
  };
}

function defaultConditionBranchWhen(): JsonRecord {
  return { value: valueRef("params", ["value"]), equals: true };
}

function switchCaseMatchDraft(switchCase: JsonRecord): {
  kind: WorkflowEdgeEditorMatchKind;
  value: unknown;
} {
  if ("when" in switchCase) {
    return { kind: "when", value: switchCase.when ?? {} };
  }

  if ("condition" in switchCase) {
    return { kind: "when", value: switchCase.condition ?? {} };
  }

  if ("not_equals" in switchCase) {
    return { kind: "not_equals", value: switchCase.not_equals };
  }

  if ("exists" in switchCase) {
    return { kind: "exists", value: switchCase.exists };
  }

  return { kind: "equals", value: "equals" in switchCase ? switchCase.equals : true };
}

function stringifyJson(value: unknown): string {
  return JSON.stringify(value ?? null, null, 2);
}

function parseWorkflowEdgeDraftValues(
  draft: WorkflowEdgeEditorDraft,
): { ok: true; when?: JsonRecord; matchValue?: unknown } | { ok: false; message: string } {
  if (isConditionBranchOption(draft.optionId)) {
    const when = parseDraftJson(draft.whenJson);

    if (!when.ok) {
      return { ok: false, message: "Condition branch predicate must be valid JSON" };
    }

    if (!isRecord(when.value)) {
      return { ok: false, message: "Condition branch predicate must be a JSON object" };
    }

    return { ok: true, when: when.value };
  }

  if (isSwitchCaseOption(draft.optionId)) {
    const match = parseDraftJson(draft.matchJson);

    if (!match.ok) {
      return { ok: false, message: "Switch case match must be valid JSON" };
    }

    return { ok: true, matchValue: match.value };
  }

  return { ok: true };
}

function isConditionBranchOption(optionId: string): boolean {
  return optionId.startsWith("branch:");
}

function isSwitchCaseOption(optionId: string): boolean {
  return optionId.startsWith("control:cases:");
}

function parseDraftJson(text: string): { ok: true; value: unknown } | { ok: false } {
  try {
    return { ok: true, value: JSON.parse(text) };
  } catch {
    return { ok: false };
  }
}

function writeWorkflowEdgeDraft(
  node: JsonRecord,
  draft: WorkflowEdgeEditorDraft,
  parsed: { ok: true; when?: JsonRecord; matchValue?: unknown },
): string | null {
  if (draft.optionId.startsWith("direct:")) {
    const key = draft.optionId.slice("direct:".length) as WorkflowDirectTransitionKey;

    if (!directTransitionKeys.includes(key)) {
      return null;
    }

    const transitions = asRecord(node.transitions);
    node.transitions = transitions;
    transitions[key] = nodeRef(draft.target);
    return key;
  }

  if (draft.optionId.startsWith("branch:")) {
    const transitions = asRecord(node.transitions);
    node.transitions = transitions;
    const branches = asArray(transitions.branches);
    transitions.branches = branches;
    const index = edgeOptionIndex(draft.optionId, "branch", branches.length);

    if (index === null) {
      return null;
    }

    const previous = asRecord(branches[index]);
    const branch: JsonRecord = {
      ...previous,
      when: parsed.when ?? (isRecord(previous.when) ? previous.when : defaultConditionBranchWhen()),
      target: nodeRef(draft.target),
    };
    applyOptionalLabel(branch, draft.label);
    applyBranchPriority(branch, draft, branches, index);
    branches[index] = branch;
    return `branches.${String(index)}`;
  }

  if (!draft.optionId.startsWith("control:")) {
    return null;
  }

  const parameters = asRecord(node.parameters);
  node.parameters = parameters;
  const controlParts = draft.optionId.split(":");

  if (!controlParts[1]) {
    return null;
  }

  const parameterKey = controlParts[1];

  if (controlParts.length > 2) {
    const rawIndex = controlParts[2];
    const list = asArray(parameters[parameterKey]);
    parameters[parameterKey] = list;
    const index = rawIndex === "new" ? list.length : Number(rawIndex);

    if (!Number.isInteger(index) || index < 0) {
      return null;
    }

    if (parameterKey === "cases") {
      const previous = asRecord(list[index]);
      const keysToRemove = new Set(["equals", "not_equals", "exists", "when", "condition"]);
      const switchCase: JsonRecord = Object.fromEntries(
        Object.entries({ ...previous, target: nodeRef(draft.target) }).filter(
          ([key]) => !keysToRemove.has(key),
        ),
      );

      switchCase[draft.matchKind] = asJsonValue(parsed.matchValue ?? true);
      applyOptionalLabel(switchCase, draft.label);
      list[index] = switchCase;
    } else if (parameterKey === "buckets") {
      // preserve the bucket's weight; only its target is edited from the canvas.
      const previous = asRecord(list[index]);
      list[index] = { ...previous, target: nodeRef(draft.target) };
    } else {
      list[index] = nodeRef(draft.target);
    }

    return parameterSemanticKey(parameterKey, index);
  }

  parameters[parameterKey] = nodeRef(draft.target);
  return parameterSemanticKey(parameterKey);
}

function applyOptionalLabel(record: JsonRecord, label: string) {
  const trimmed = label.trim();

  if (trimmed) {
    record.label = trimmed;
  } else {
    delete record.label;
  }
}

// write a predicate edge's selection priority (lower is evaluated first). an unset draft priority
// on a new branch defaults to the next free slot after the highest existing priority.
function applyBranchPriority(
  branch: JsonRecord,
  draft: WorkflowEdgeEditorDraft,
  branches: unknown[],
  index: number,
) {
  if (typeof draft.priority === "number" && Number.isFinite(draft.priority)) {
    branch.priority = Math.trunc(draft.priority);
    return;
  }

  const isNew = index >= branches.length || !isRecord(branches[index]);

  if (!isNew) {
    delete branch.priority;
    return;
  }

  const highest = branches.reduce<number>((max, item) => {
    const value = isRecord(item) && typeof item.priority === "number" ? item.priority : null;
    return value !== null && value > max ? value : max;
  }, 0);
  branch.priority = highest + 1;
}

function edgeOptionIndex(optionId: string, prefix: string, newIndex: number): number | null {
  const raw = optionId.slice(prefix.length + 1);

  if (raw === "new") {
    return newIndex;
  }

  const index = Number(raw);
  return Number.isInteger(index) && index >= 0 ? index : null;
}

function orderedEdgeLocation(
  definition: JsonRecord,
  draft: WorkflowEdgeEditorDraft,
): null | {
  items: unknown[];
  index: number;
  semanticKey: (index: number) => string;
  optionId: (index: number) => string;
} {
  const nodes = recordArray(definition.nodes);
  const node = nodes.find((item: JsonRecord) => String(item.id) === draft.source);

  if (!node) {
    return null;
  }

  if (draft.optionId.startsWith("branch:")) {
    const rawBranches = asRecord(node.transitions).branches;
    const branches = Array.isArray(rawBranches) ? (rawBranches as unknown[]) : null;
    const index = edgeOptionIndex(draft.optionId, "branch", -1);

    if (!branches || index === null) {
      return null;
    }

    return {
      items: branches,
      index,
      semanticKey: (nextIndex) => `branches.${String(nextIndex)}`,
      optionId: (nextIndex) => `branch:${String(nextIndex)}`,
    };
  }

  if (!draft.optionId.startsWith("control:")) {
    return null;
  }

  const [, parameterKey, rawIndex] = draft.optionId.split(":");

  if (
    !["cases", "branches", "wait_for"].includes(parameterKey) ||
    rawIndex === "new"
  ) {
    return null;
  }

  const rawItems = asRecord(node.parameters)[parameterKey];
  const items = Array.isArray(rawItems) ? (rawItems as unknown[]) : null;
  const index = Number(rawIndex);

  if (!items || !Number.isInteger(index) || index < 0) {
    return null;
  }

  return {
    items,
    index,
    semanticKey: (nextIndex) => parameterSemanticKey(parameterKey, nextIndex),
    optionId: (nextIndex) => `control:${parameterKey}:${String(nextIndex)}`,
  };
}

export function applyWorkflowEdgeSemantic(
  node: JsonRecord,
  target: string,
  optionId: string,
): string | null {
  if (!target) {
    return null;
  }

  if (optionId.startsWith("direct:")) {
    const key = optionId.slice("direct:".length) as WorkflowDirectTransitionKey;

    if (!directTransitionKeys.includes(key)) {
      return null;
    }

    const transitions = asRecord(node.transitions);
    node.transitions = transitions;
    transitions[key] = nodeRef(target);
    return key;
  }

  if (optionId.startsWith("branch:")) {
    const transitions = asRecord(node.transitions);
    node.transitions = transitions;
    const branches = asArray(transitions.branches);
    transitions.branches = branches;
    const rawIndex = optionId.slice("branch:".length);
    const index = rawIndex === "new" ? branches.length : Number(rawIndex);

    if (!Number.isInteger(index) || index < 0) {
      return null;
    }

    const previous = asRecord(branches[index]);
    branches[index] = {
      when: isRecord(previous.when)
        ? previous.when
        : { value: valueRef("params", ["value"]), equals: true },
      target: nodeRef(target),
    };
    return `branches.${String(index)}`;
  }

  if (!optionId.startsWith("control:")) {
    return null;
  }

  const parameters = asRecord(node.parameters);
  node.parameters = parameters;
  const controlParts = optionId.split(":");

  if (!controlParts[1]) {
    return null;
  }

  const parameterKey = controlParts[1];

  if (controlParts.length > 2) {
    const rawIndex = controlParts[2];
    const list = asArray(parameters[parameterKey]);
    parameters[parameterKey] = list;
    const index = rawIndex === "new" ? list.length : Number(rawIndex);

    if (!Number.isInteger(index) || index < 0) {
      return null;
    }

    if (parameterKey === "cases") {
      const previous = asRecord(list[index]);
      const switchCase: JsonRecord = { ...previous, target: nodeRef(target) };
      list[index] = switchCase;

      if (
        !("equals" in switchCase) &&
        !("not_equals" in switchCase) &&
        !("exists" in switchCase) &&
        !("when" in switchCase)
      ) {
        switchCase.equals = true;
      }
    } else {
      list[index] = nodeRef(target);
    }

    return parameterSemanticKey(parameterKey, index);
  }

  parameters[parameterKey] = nodeRef(target);
  return parameterSemanticKey(parameterKey);
}

export function autoArrangeWorkflowLayout(
  definition: JsonRecord,
  direction: WorkflowLayoutDirection = "horizontal",
): Record<string, WorkflowLayoutPosition> {
  const nodes = recordArray(definition.nodes);
  const ids = nodes
    .map((node: JsonRecord, index: number) => displayValue(node.id) || `step_${String(index + 1)}`)
    .filter(Boolean);

  if (ids.length === 0) {
    return {};
  }

  const nodeIds = new Set(ids);
  const indexById = new Map(ids.map((id, index) => [id, index]));
  const edges = workflowLayoutEdges(nodes, nodeIds);
  const components = stronglyConnectedComponents(ids, edges);
  const componentById = new Map<string, number>();
  components.forEach((component, componentIndex) => {
    for (const id of component) {
      componentById.set(id, componentIndex);
    }
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

    if (
      sourceComponent === undefined ||
      targetComponent === undefined ||
      sourceComponent === targetComponent
    ) {
      continue;
    }

    const targets = componentEdges.get(sourceComponent);

    if (!targets) {
      continue;
    }

    if (targets.has(targetComponent)) {
      continue;
    }

    targets.add(targetComponent);
    incomingCounts.set(targetComponent, (incomingCounts.get(targetComponent) ?? 0) + 1);
  }

  const levels = componentLevels(
    components,
    componentEdges,
    incomingCounts,
    definition.start,
    indexById,
  );
  const maxLevel = Math.max(0, ...levels);
  const grouped = Array.from({ length: maxLevel + 1 }, () => [] as number[]);
  levels.forEach((level, componentIndex) => grouped[level].push(componentIndex));

  for (const group of grouped) {
    group.sort(
      (left, right) =>
        componentSortKey(components[left], indexById) -
        componentSortKey(components[right], indexById),
    );
  }

  const columnGap = 270;
  const rowGap = 150;
  const levelSlots = grouped.map((group) =>
    group.reduce((total, componentIndex) => total + components[componentIndex].length, 0),
  );
  const maxSlots = Math.max(1, ...levelSlots);
  const positions: Record<string, WorkflowLayoutPosition> = {};

  grouped.forEach((group, level) => {
    let slot = 0;
    const yOffset = ((maxSlots - levelSlots[level]) * rowGap) / 2;

    for (const componentIndex of group) {
      const component = [...components[componentIndex]].sort(
        (left, right) => (indexById.get(left) ?? 0) - (indexById.get(right) ?? 0),
      );

      for (const id of component) {
        const primary = level * columnGap;
        const secondary = yOffset + slot * rowGap;
        positions[id] =
          direction === "vertical" ? { x: secondary, y: primary } : { x: primary, y: secondary };
        slot += 1;
      }
    }
  });

  return positions;
}

export function autoArrangeWorkflowEdgeHandles(
  definition: JsonRecord,
  positions: Record<string, WorkflowLayoutPosition>,
) {
  const nodes = recordArray(definition.nodes);
  const nodeIds = new Set(nodes.map((node: JsonRecord) => String(node.id)).filter(Boolean));

  const setHandles = (source: string, semanticKey: string, target: string | null) => {
    if (!target || !nodeIds.has(source) || !nodeIds.has(target)) {
      return;
    }

    const handles = connectionHandlesForPositions(positions[source], positions[target]);
    const style = edgeHandles(definition, source, semanticKey).edgeStyle;
    setWorkflowEdgeHandles(
      definition,
      source,
      semanticKey,
      handles.sourceHandle,
      handles.targetHandle,
      style,
    );
  };

  for (const node of nodes) {
    const source = displayValue(node.id);
    const transitions = isRecord(node.transitions) ? node.transitions : {};

    for (const key of directTransitionKeys) {
      setHandles(source, key, nodeRefId(transitions[key]));
    }

    recordArray(transitions.branches).forEach((branch, index) => {
      setHandles(source, `branches.${String(index)}`, nodeRefId(branch.target));
    });

    for (const { target, parameterKey, parameterIndex } of controlFlowTargetValues(node)) {
      setHandles(source, parameterSemanticKey(parameterKey, parameterIndex), target);
    }
  }
}

export function createWorkflowNode(
  kind: WorkflowNodeKind,
  nodes: JsonRecord[],
): WorkflowEditorNodeRecord {
  const id = uniqueWorkflowNodeId(nodes, kind);
  const node: WorkflowEditorNodeRecord = {
    id,
    kind,
    parameters: {},
    retry: { max_attempts: 1 },
    transitions: {},
  };

  switch (kind) {
    case "action":
      node.action = { provider: "", function: "", timeout_seconds: 300, configuration: {} };
      break;
    case "approval":
      node.parameters = { approval_type: "generic", prompt: "Approval required" };
      node.transitions = { on_success: nodeRef("end"), on_reject: nodeRef("end") };
      break;
    case "gate":
      node.parameters = { kind: "manual", poll_interval: 30 };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "signal":
      node.parameters = { name: "signal" };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "loop":
      node.parameters = { items: [], target: nodeRef("end") };
      node.max_iterations = 10;
      break;
    case "condition":
      node.condition = {};
      node.transitions = {
        branches: [
          {
            when: { value: valueRef("params", ["approved"]), equals: true },
            target: nodeRef("end"),
          },
        ],
        next: nodeRef("end"),
      };
      break;
    case "wait":
      node.wait = { seconds: 60 };
      break;
    case "switch":
      node.parameters = { value: valueRef("params", ["mode"]), cases: [], default: nodeRef("end") };
      break;
    case "toggle":
      node.parameters = {
        value: valueRef("config", ["flags", "enabled"]),
        on: nodeRef("end"),
        off: nodeRef("end"),
      };
      break;
    case "percentage":
      node.parameters = {
        key: valueRef("input", ["user_id"]),
        buckets: [],
        default: nodeRef("end"),
      };
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
    case "output":
      node.parameters = { event_type: "workflow.output", data: {} };
      break;
    case "input":
      node.parameters = { prompt: "Provide input" };
      break;
    case "config":
      node.parameters = { name: "", metadata: {} };
      break;
    case "subflow":
      node.subflow_id = "";
      break;
    case "assert":
      node.parameters = { assertions: [] };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "transform":
      node.parameters = { bindings: {} };
      node.transitions = { next: nodeRef("end") };
      break;
    case "audit":
      node.parameters = { action: "" };
      node.transitions = { next: nodeRef("end") };
      break;
    case "checkpoint":
      node.parameters = { name: "checkpoint" };
      node.transitions = { next: nodeRef("end") };
      break;
    case "mutex":
      node.parameters = { name: "my-mutex" };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "throttle":
      node.parameters = { name: "my-throttle", max_per_window: 10, window_seconds: 60 };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "await_run":
      node.parameters = { run_ids: [], mode: "all" };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "debounce":
      node.parameters = { name: "my-debounce", delay_seconds: 30 };
      node.transitions = { on_success: nodeRef("end") };
      break;
    case "collect":
      node.parameters = { name: "my-collect", max: 10 };
      node.transitions = { on_success: nodeRef("end") };
      break;
    case "barrier":
      node.parameters = { name: "my-barrier", count: 2 };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "circuit_breaker":
      node.parameters = {
        name: "my-circuit-breaker",
        threshold: 5,
        window_seconds: 60,
        cooldown_seconds: 30,
      };
      node.transitions = { on_success: nodeRef("end"), on_failure: nodeRef("end") };
      break;
    case "event_source":
      node.parameters = { event_type: "" };
      node.transitions = { on_success: nodeRef("end") };
      break;
  }

  return node;
}

export function uniqueWorkflowNodeId(nodes: JsonRecord[], base: string): string {
  return uniqueNodeId(
    base.replace(/[^a-zA-Z0-9_]+/g, "_") || "node",
    new Set(nodes.map((node) => String(node.id)).filter(Boolean)),
  );
}

export function addDirectTransition(
  node: JsonRecord,
  target: string,
  preferredKey?: string | null,
): WorkflowDirectTransitionKey {
  const key = directTransitionKeys.includes(preferredKey as WorkflowDirectTransitionKey)
    ? (preferredKey as WorkflowDirectTransitionKey)
    : firstAvailableTransition(node);
  const transitions = asRecord(node.transitions);
  node.transitions = transitions;
  transitions[key] = nodeRef(target);
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
    connection.sourceHandle === connection.targetHandle,
  );
}

export function setWorkflowEdgeHandles(
  definition: JsonRecord,
  source: string,
  semanticKey: string,
  sourceHandle?: string | null,
  targetHandle?: string | null,
  edgeStyle?: WorkflowEdgeStyle | null,
  labelOffset?: WorkflowEdgeLabelOffset | null,
  labelAnchor?: WorkflowEdgeLabelAnchor | null,
) {
  const ui = asRecord(definition.ui);
  definition.ui = ui;
  const edgeHandles = asRecord(ui.edge_handles);
  ui.edge_handles = edgeHandles;
  const key = edgeHandleKey(source, semanticKey);
  // missing label metadata preserves manual placement; null/default clears it.
  const previousOffset = normalizeLabelOffset(asRecord(edgeHandles[key]).labelOffset);
  const nextOffset = labelOffset === undefined ? previousOffset : normalizeLabelOffset(labelOffset);
  const previousAnchor = normalizeLabelAnchor(asRecord(edgeHandles[key]).labelAnchor);
  const nextAnchor = labelAnchor === undefined ? previousAnchor : normalizeLabelAnchor(labelAnchor);
  edgeHandles[key] = asJsonValue({
    sourceHandle: normalizeConnectionHandle(sourceHandle),
    targetHandle: normalizeConnectionHandle(targetHandle),
    edgeStyle: normalizeWorkflowEdgeStyle(edgeStyle),
    ...(nextOffset ? { labelOffset: nextOffset } : {}),
    ...(nextAnchor ? { labelAnchor: nextAnchor } : {}),
  });
}

export function removeWorkflowEdgeHandles(
  definition: JsonRecord,
  source: string,
  semanticKey: string,
) {
  const handles = asRecord(definition.ui).edge_handles;

  if (!isRecord(handles)) {
    return;
  }

  const key = edgeHandleKey(source, semanticKey);
  asRecord(definition.ui).edge_handles = Object.fromEntries(
    Object.entries(handles).filter(([entryKey]) => entryKey !== key),
  );
}

export function workflowEdgeSemanticKey(edge: GraphEdgeLike): string {
  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (data?.transitionKey) {
    return data.transitionKey;
  }

  if (typeof data?.branchIndex === "number") {
    return `branches.${String(data.branchIndex)}`;
  }

  return parameterSemanticKey(data?.parameterKey, data?.parameterIndex);
}

export function setWorkflowEdgeLabelOffset(
  definition: JsonRecord,
  edge: GraphEdgeLike,
  labelOffset: WorkflowEdgeLabelOffset | null,
) {
  const data = edge.data as WorkflowEditorEdgeData | undefined;
  const semanticKey = workflowEdgeSemanticKey(edge);

  if (!semanticKey) {
    return;
  }

  setWorkflowEdgeHandles(
    definition,
    edge.source,
    semanticKey,
    edge.sourceHandle,
    edge.targetHandle,
    normalizeWorkflowEdgeStyle(data?.edgeStyle),
    labelOffset,
  );
}

export function setWorkflowEdgeLabelAnchor(
  definition: JsonRecord,
  edge: GraphEdgeLike,
  labelAnchor: WorkflowEdgeLabelAnchor | null,
) {
  const data = edge.data as WorkflowEditorEdgeData | undefined;
  const semanticKey = workflowEdgeSemanticKey(edge);

  if (!semanticKey) {
    return;
  }

  setWorkflowEdgeHandles(
    definition,
    edge.source,
    semanticKey,
    edge.sourceHandle,
    edge.targetHandle,
    normalizeWorkflowEdgeStyle(data?.edgeStyle),
    undefined,
    labelAnchor,
  );
}

function removeEdgeHandlesForEdge(definition: JsonRecord, edge: GraphEdgeLike) {
  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (data?.transitionKey) {
    removeWorkflowEdgeHandles(definition, edge.source, data.transitionKey);
  }

  if (typeof data?.branchIndex === "number") {
    removeWorkflowEdgeHandles(definition, edge.source, `branches.${String(data.branchIndex)}`);
  }

  if (data?.parameterKey) {
    removeWorkflowEdgeHandles(
      definition,
      edge.source,
      parameterSemanticKey(data.parameterKey, data.parameterIndex),
    );
  }
}

function swapWorkflowEdgeHandles(
  definition: JsonRecord,
  source: string,
  leftSemanticKey: string,
  rightSemanticKey: string,
) {
  const handles = asRecord(definition.ui).edge_handles;

  if (!isRecord(handles)) {
    return;
  }

  const leftKey = edgeHandleKey(source, leftSemanticKey);
  const rightKey = edgeHandleKey(source, rightSemanticKey);
  const left = handles[leftKey];
  const right = handles[rightKey];
  let next = { ...handles };

  if (right === undefined) {
    next = Object.fromEntries(Object.entries(next).filter(([entryKey]) => entryKey !== leftKey));
  } else {
    next[leftKey] = right;
  }

  if (left === undefined) {
    next = Object.fromEntries(Object.entries(next).filter(([entryKey]) => entryKey !== rightKey));
  } else {
    next[rightKey] = left;
  }

  asRecord(definition.ui).edge_handles = next;
}

function renameWorkflowEdgeHandleSource(
  definition: JsonRecord,
  previousId: string,
  nextId: string,
) {
  const handles = asRecord(definition.ui).edge_handles;

  if (!isRecord(handles)) {
    return;
  }

  const prefix = `${previousId}:`;

  let next = { ...handles };

  for (const key of Object.keys(handles)) {
    if (!key.startsWith(prefix)) {
      continue;
    }

    const nextKey = `${nextId}:${key.slice(prefix.length)}`;

    if (next[nextKey] === undefined) {
      next[nextKey] = next[key];
    }

    next = Object.fromEntries(Object.entries(next).filter(([entryKey]) => entryKey !== key));
  }

  asRecord(definition.ui).edge_handles = next;
}

export function removeEditableEdge(node: JsonRecord, edge: GraphEdgeLike): boolean {
  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (!data?.editable || !isRecord(node.transitions)) {
    return false;
  }

  if (
    data.kind === "direct" &&
    data.transitionKey &&
    nodeRefId(node.transitions[data.transitionKey]) === edge.target
  ) {
    node.transitions = Object.fromEntries(
      Object.entries(node.transitions).filter(([key]) => key !== data.transitionKey),
    );
    return true;
  }

  if (
    data.kind === "branch" &&
    typeof data.branchIndex === "number" &&
    Array.isArray(node.transitions.branches)
  ) {
    const branch = asRecord(asArray(node.transitions.branches)[data.branchIndex]);

    if (nodeRefId(branch.target) !== edge.target) {
      return false;
    }

    node.transitions.branches.splice(data.branchIndex, 1);
    return true;
  }

  return false;
}

export function removeWorkflowEdge(node: JsonRecord, edge: GraphEdgeLike): boolean {
  if (removeEditableEdge(node, edge)) {
    return true;
  }

  const data = edge.data as WorkflowEditorEdgeData | undefined;

  if (data?.kind !== "control" || !data.parameterKey) {
    return false;
  }

  const parameters = asRecord(node.parameters);

  if (typeof data.parameterIndex === "number" && Array.isArray(parameters[data.parameterKey])) {
    const list = asArray(parameters[data.parameterKey]);
    const current = list[data.parameterIndex];

    if (nodeRefId(current) !== edge.target && nodeRefId(asRecord(current).target) !== edge.target) {
      return false;
    }

    list.splice(data.parameterIndex, 1);
    return true;
  }

  if (nodeRefId(parameters[data.parameterKey]) === edge.target) {
    node.parameters = Object.fromEntries(
      Object.entries(parameters).filter(([key]) => key !== data.parameterKey),
    );
    return true;
  }

  return false;
}

export function removeWorkflowNodeReferences(definition: JsonRecord, nodeId: string) {
  const nodes = recordArray(definition.nodes);

  for (const node of nodes) {
    const transitions = isRecord(node.transitions) ? node.transitions : {};

    let currentTransitions = transitions;

    for (const key of directTransitionKeys) {
      if (nodeRefId(currentTransitions[key]) === nodeId) {
        currentTransitions = Object.fromEntries(
          Object.entries(currentTransitions).filter(([entryKey]) => entryKey !== key),
        );
        node.transitions = currentTransitions;
      }
    }

    if (Array.isArray(transitions.branches)) {
      transitions.branches = recordArray(transitions.branches).filter(
        (branch) => nodeRefId(branch.target) !== nodeId,
      );
    }

    const parameters = isRecord(node.parameters) ? node.parameters : {};

    let currentParameters = parameters;

    for (const key of ["default", "body", "catch", "finally", "target"]) {
      if (nodeRefId(currentParameters[key]) === nodeId) {
        currentParameters = Object.fromEntries(
          Object.entries(currentParameters).filter(([entryKey]) => entryKey !== key),
        );
        node.parameters = currentParameters;
      }
    }

    for (const key of ["branches", "wait_for", "cases"]) {
      if (!Array.isArray(parameters[key])) {
        continue;
      }

      parameters[key] = parameters[key].filter(
        (item: unknown) =>
          nodeRefId(item) !== nodeId && nodeRefId((item as JsonRecord).target) !== nodeId,
      );
    }
  }
}

export function setConditionBranch(
  node: JsonRecord,
  index: number,
  when: JsonRecord,
  target: string,
) {
  const transitions = asRecord(node.transitions);
  node.transitions = transitions;
  const branches = asArray(transitions.branches);
  transitions.branches = branches;
  branches[index] = { when, target: nodeRef(target) };
}

export function removeConditionBranch(node: JsonRecord, index: number) {
  if (!isRecord(node.transitions) || !Array.isArray(node.transitions.branches)) {
    return;
  }

  node.transitions.branches.splice(index, 1);
}

export function normalizeWorkflowDefinition(workflow: WorkflowDefinition): WorkflowDefinition {
  const definition = normalizeDefinition(workflow.definition);
  return { ...workflow, definition };
}

export function workflowLayoutNodes(definition: JsonRecord): JsonRecord {
  const layout = asRecord(definition.ui).layout;

  if (!isRecord(layout)) {
    return {};
  }

  if (isRecord(layout.nodes)) {
    return layout.nodes;
  }

  return layout;
}

function normalizeDefinition(definition: JsonRecord): JsonRecord {
  const nextDefinition = cloneRecord(definition);
  normalizeLayout(nextDefinition);
  const nodes = recordArray(nextDefinition.nodes);
  nextDefinition.nodes = nodes;

  const ids = new Set(nodes.map((node: JsonRecord) => String(node.id)).filter(Boolean));
  ensureEndNode(nodes, ids);
  ensureFailNode(nodes, ids);
  const startId = ensureStartNode(nodes, ids);
  nextDefinition.start = startId;
  return nextDefinition;
}

function normalizeLayout(definition: JsonRecord) {
  const layout = asRecord(definition.ui).layout;

  if (!isRecord(layout)) {
    return;
  }

  const directEntries = Object.entries(layout).filter(
    ([key, value]) => key !== "nodes" && isRecord(value),
  );

  if (directEntries.length === 0) {
    return;
  }

  const layoutNodes = asRecord(layout.nodes);
  layout.nodes = layoutNodes;

  for (const [id, position] of directEntries) {
    layoutNodes[id] ??= position;
  }

  const keysToRemove = new Set(directEntries.map(([key]) => key));
  const nextLayout = Object.fromEntries(
    Object.entries(layout).filter(([key]) => !keysToRemove.has(key)),
  );
  asRecord(definition.ui).layout = nextLayout;
}

function ensureEndNode(nodes: JsonRecord[], ids: Set<string>): string {
  const existing = firstNodeId(nodes, (kind) => kind === "end");

  if (existing) {
    return existing;
  }

  const id = uniqueNodeId("end", ids);
  nodes.push({ id, kind: "end" });
  return id;
}

function ensureFailNode(nodes: JsonRecord[], ids: Set<string>): string {
  const existing = firstNodeId(nodes, (kind) => kind === "fail");

  if (existing) {
    return existing;
  }

  const id = uniqueNodeId("fail", ids);
  nodes.push({ id, kind: "fail" });
  return id;
}

function ensureStartNode(nodes: JsonRecord[], ids: Set<string>): string {
  const existing = firstNodeId(nodes, (kind) => kind === "start");

  if (existing) {
    return existing;
  }

  const id = uniqueNodeId("start", ids);
  nodes.unshift({
    id,
    kind: "start",
    transitions: {},
  });
  return id;
}

function hasSuccessTransition(node: JsonRecord): boolean {
  const transitions = node.transitions;
  return Boolean(
    (isRecord(transitions) &&
      (nodeRefId(transitions.next) ??
        nodeRefId(transitions.on_success) ??
        (Array.isArray(transitions.branches) && transitions.branches.length > 0))) ||
    controlFlowTargetValues(node).length > 0,
  );
}

function inferredNodeStatus(
  node: JsonRecord,
  id: string,
  detail: WorkflowRunDetail | null,
): string | undefined {
  if (!detail) {
    return undefined;
  }

  if (detail.run.active_node_id === id && isWorkflowRunDisplayStatus(detail.run.status)) {
    return detail.run.status;
  }

  if (
    node.kind === "end" &&
    detail.run.active_node_id === id &&
    detail.run.status === "succeeded"
  ) {
    return "succeeded";
  }

  if (node.kind === "fail" && detail.run.active_node_id === id && detail.run.status === "failed") {
    return "failed";
  }

  if (node.kind === "start" && detail.nodes.length > 0) {
    return "succeeded";
  }

  return undefined;
}

function isWorkflowRunDisplayStatus(status: string | undefined): status is string {
  return [
    "queued",
    "running",
    "debug_paused",
    "waiting",
    "approval_required",
    "blocked",
    "succeeded",
    "failed",
    "timed_out",
    "canceled",
  ].includes(status ?? "");
}

function firstNodeId(nodes: JsonRecord[], predicate: (kind?: string) => boolean): string | null {
  const node = nodes.find((item) =>
    predicate(typeof item.kind === "string" ? item.kind : undefined),
  );
  return node?.id ? displayValue(node.id) : null;
}

function uniqueNodeId(base: string, ids: Set<string>): string {
  if (!ids.has(base)) {
    ids.add(base);
    return base;
  }

  for (let index = 2; ; index += 1) {
    const candidate = `${base}_${String(index)}`;

    if (!ids.has(candidate)) {
      ids.add(candidate);
      return candidate;
    }
  }
}

function cloneRecord(value: JsonRecord): JsonRecord {
  return JSON.parse(JSON.stringify(value)) as JsonRecord;
}

export function isRecord(value: unknown): value is JsonRecord {
  return isJsonRecord(value);
}

// coerce unknown json into a mutable record/array so in-place graph edits stay
// type-safe. returns the same reference when the value already matches, so
// reassigning the coerced value back onto its parent preserves mutation.
export function asRecord(value: unknown): JsonRecord {
  return asJsonRecord(value);
}

export function recordArray(value: unknown): JsonRecord[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter(isRecord);
}

export function asArray(value: unknown): JsonRecord[] {
  return recordArray(value);
}

function graphEdge(
  source: string,
  target: string,
  label: string,
  data: WorkflowEditorEdgeData,
): GraphEdgeModel {
  const edgeLabel = data.validationCount ? `${label} !` : label;
  const edgeStyle = normalizeWorkflowEdgeStyle(data.edgeStyle);
  const labelOffset = normalizeLabelOffset(data.labelOffset);
  const labelAnchor = normalizeLabelAnchor(data.labelAnchor);
  return {
    id: edgeId(source, target, label, data),
    type: "workflow",
    source,
    target,
    sourceHandle: data.sourceHandle,
    targetHandle: data.targetHandle,
    label: edgeLabel,
    data: { ...data, edgeStyle, labelOffset, labelAnchor },
    updatable: data.editable,
    interactionWidth: 24,
  };
}

function workflowLayoutEdges(
  nodes: JsonRecord[],
  nodeIds: Set<string>,
): { source: string; target: string }[] {
  const edges: { source: string; target: string }[] = [];

  for (const node of nodes) {
    const source = displayValue(node.id);

    if (!source || !nodeIds.has(source)) {
      continue;
    }

    const transitions = isRecord(node.transitions) ? node.transitions : {};

    for (const key of directTransitionKeys) {
      pushLayoutEdge(edges, source, nodeRefId(transitions[key]), nodeIds);
    }

    for (const branch of asArray(transitions.branches)) {
      pushLayoutEdge(edges, source, nodeRefId(asRecord(branch).target), nodeIds);
    }

    for (const edge of parameterLayoutEdges(node, source, nodeIds)) {
      edges.push(edge);
    }
  }

  return dedupeLayoutEdges(edges);
}

function parameterLayoutEdges(
  node: JsonRecord,
  source: string,
  nodeIds: Set<string>,
): { source: string; target: string }[] {
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  const edges: { source: string; target: string }[] = [];

  switch (node.kind) {
    case "switch": {
      const cases = recordArray(parameters.cases);

      for (const item of cases.filter(isRecord)) {
        pushLayoutEdge(edges, source, nodeRefId(item.target), nodeIds);
      }

      pushLayoutEdge(edges, source, nodeRefId(parameters.default), nodeIds);
      return edges;
    }

    case "toggle":
      pushLayoutEdge(edges, source, nodeRefId(parameters.on), nodeIds);
      pushLayoutEdge(edges, source, nodeRefId(parameters.off), nodeIds);
      return edges;

    case "percentage": {
      const buckets = asArray(parameters.buckets);

      for (const item of buckets.filter(isRecord)) {
        pushLayoutEdge(edges, source, nodeRefId(item.target), nodeIds);
      }

      pushLayoutEdge(edges, source, nodeRefId(parameters.default), nodeIds);
      return edges;
    }

    case "parallel":
    case "race":
      for (const target of nodeRefArray(parameters.branches)) {
        pushLayoutEdge(edges, source, target, nodeIds);
      }

      return edges;
    case "join":
      for (const dependency of nodeRefArray(parameters.wait_for)) {
        pushLayoutEdge(edges, dependency, source, nodeIds);
      }

      return edges;
    case "try":
      for (const key of ["body", "catch", "finally"]) {
        pushLayoutEdge(edges, source, nodeRefId(parameters[key]), nodeIds);
      }

      return edges;
    case "loop":
    case "map":
      pushLayoutEdge(edges, source, nodeRefId(parameters.target), nodeIds);
      return edges;
    default:
      return edges;
  }
}

function pushLayoutEdge(
  edges: { source: string; target: string }[],
  source: string,
  target: string | null,
  nodeIds: Set<string>,
) {
  if (!target || source === target || !nodeIds.has(source) || !nodeIds.has(target)) {
    return;
  }

  edges.push({ source, target });
}

function dedupeLayoutEdges(
  edges: { source: string; target: string }[],
): { source: string; target: string }[] {
  const seen = new Set<string>();
  return edges.filter((edge) => {
    const key = `${edge.source}\u0000${edge.target}`;

    if (seen.has(key)) {
      return false;
    }

    seen.add(key);
    return true;
  });
}

function stronglyConnectedComponents(
  ids: string[],
  edges: { source: string; target: string }[],
): string[][] {
  const adjacency = new Map(ids.map((id) => [id, [] as string[]]));

  for (const edge of edges) {
    adjacency.get(edge.source)?.push(edge.target);
  }

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
        const currentLow = lowLinkById.get(id);
        const targetLow = lowLinkById.get(target);

        if (currentLow !== undefined && targetLow !== undefined) {
          lowLinkById.set(id, Math.min(currentLow, targetLow));
        }
      } else if (onStack.has(target)) {
        const currentLow = lowLinkById.get(id);
        const targetIndex = indexById.get(target);

        if (currentLow !== undefined && targetIndex !== undefined) {
          lowLinkById.set(id, Math.min(currentLow, targetIndex));
        }
      }
    }

    if (lowLinkById.get(id) !== indexById.get(id)) {
      return;
    }

    const component: string[] = [];

    for (;;) {
      const member = stack.pop();

      if (!member) {
        break;
      }

      onStack.delete(member);
      component.push(member);

      if (member === id) {
        break;
      }
    }

    components.push(component);
  }

  for (const id of ids) {
    if (!indexById.has(id)) {
      visit(id);
    }
  }

  return components;
}

function componentLevels(
  components: string[][],
  componentEdges: Map<number, Set<number>>,
  incomingCounts: Map<number, number>,
  start: unknown,
  indexById: Map<string, number>,
): number[] {
  const levels = components.map(() => 0);
  const startComponent =
    typeof start === "string" ? components.findIndex((component) => component.includes(start)) : -1;
  const queue = [...components.keys()]
    .filter((componentIndex) => incomingCounts.get(componentIndex) === 0)
    .sort((left, right) => {
      if (left === startComponent) {
        return -1;
      }

      if (right === startComponent) {
        return 1;
      }

      return (
        componentSortKey(components[left], indexById) -
        componentSortKey(components[right], indexById)
      );
    });

  for (const source of queue) {
    for (const target of componentEdges.get(source) ?? []) {
      levels[target] = Math.max(levels[target], levels[source] + 1);
      incomingCounts.set(target, (incomingCounts.get(target) ?? 0) - 1);

      if (incomingCounts.get(target) === 0) {
        queue.push(target);
      }
    }
  }

  return levels;
}

function componentSortKey(component: string[], indexById: Map<string, number>): number {
  return Math.min(...component.map((id) => indexById.get(id) ?? 0));
}

function edgeId(
  source: string,
  target: string,
  label: string,
  data: WorkflowEditorEdgeData,
): string {
  return [
    source,
    data.kind,
    data.transitionKey ?? data.parameterKey ?? data.branchIndex ?? label,
    data.parameterIndex ?? "",
    data.sourceHandle ?? "",
    data.targetHandle ?? "",
    normalizeWorkflowEdgeStyle(data.edgeStyle),
    target,
  ]
    .map((part) => encodeURIComponent(String(part)))
    .join(":");
}

function separateParallelEdges(edges: GraphEdgeModel[]): GraphEdgeModel[] {
  const groups = new Map<string, GraphEdgeModel[]>();

  for (const edge of edges) {
    const key = [edge.source, edge.target, edge.sourceHandle ?? "", edge.targetHandle ?? ""].join(
      "\u0000",
    );
    const group = groups.get(key) ?? [];
    group.push(edge);
    groups.set(key, group);
  }

  return edges.map((edge) => {
    const group = groups.get(
      [edge.source, edge.target, edge.sourceHandle ?? "", edge.targetHandle ?? ""].join("\u0000"),
    ) ?? [edge];

    if (group.length === 1) {
      return edge;
    }

    const index = group.findIndex((item) => item.id === edge.id);
    const parallelOffset = 18 + index * 18;
    return {
      ...edge,
      data: { ...(edge.data as WorkflowEditorEdgeData), parallelOffset },
      pathOptions: { offset: parallelOffset, borderRadius: 8 },
      zIndex: index + 1,
    };
  });
}

function controlFlowEdges(
  definition: JsonRecord,
  node: JsonRecord,
  nodeIds: Set<string>,
  issuesByEdge = new Map<string, WorkflowValidationIssue[]>(),
): GraphEdgeModel[] {
  const source = displayValue(node.id);
  return controlFlowTargetValues(node)
    .filter(({ target }) => nodeIds.has(target))
    .map(({ target, label, parameterKey, parameterIndex }) => {
      const semanticKey = parameterSemanticKey(parameterKey, parameterIndex);
      const issues = issuesByEdge.get(edgeValidationKey(source, semanticKey)) ?? [];
      return graphEdge(source, target, label, {
        kind: "control",
        parameterKey,
        parameterIndex,
        ...edgeHandles(definition, source, semanticKey),
        validationCount: issues.length,
        validationSeverity: validationSeverity(issues),
        validationMessages: issues.map((issue) => issue.message),
        editable: true,
      });
    });
}

function controlFlowTargetValues(
  node: JsonRecord,
): { target: string; label: string; parameterKey?: string; parameterIndex?: number }[] {
  const parameters = isRecord(node.parameters) ? node.parameters : {};

  switch (node.kind) {
    case "switch": {
      const cases = recordArray(parameters.cases);
      const targets: {
        target: string;
        label: string;
        parameterKey?: string;
        parameterIndex?: number;
      }[] = cases
        .filter(isRecord)
        .map((item, index) => ({
          target: nodeRefId(item.target),
          label: displayValue(item.label) || `case ${String(index + 1)}`,
          parameterKey: "cases",
          parameterIndex: index,
        }))
        .filter(
          (
            item,
          ): item is {
            target: string;
            label: string;
            parameterKey: string;
            parameterIndex: number;
          } => Boolean(item.target),
        );
      const defaultTarget = nodeRefId(parameters.default);

      if (defaultTarget) {
        targets.push({ target: defaultTarget, label: "default", parameterKey: "default" });
      }

      return targets;
    }

    case "toggle": {
      const targets: {
        target: string;
        label: string;
        parameterKey?: string;
        parameterIndex?: number;
      }[] = [];
      const on = nodeRefId(parameters.on);

      if (on) {
        targets.push({ target: on, label: "on", parameterKey: "on" });
      }

      const off = nodeRefId(parameters.off);

      if (off) {
        targets.push({ target: off, label: "off", parameterKey: "off" });
      }

      return targets;
    }

    case "percentage": {
      const buckets = asArray(parameters.buckets);
      const targets: {
        target: string;
        label: string;
        parameterKey?: string;
        parameterIndex?: number;
      }[] = buckets
        .filter(isRecord)
        .map((item, index) => ({
          target: nodeRefId(item.target),
          label: `${String(Number(item.weight ?? 0))}%`,
          parameterKey: "buckets",
          parameterIndex: index,
        }))
        .filter(
          (
            item,
          ): item is {
            target: string;
            label: string;
            parameterKey: string;
            parameterIndex: number;
          } => Boolean(item.target),
        );
      const percentageDefault = nodeRefId(parameters.default);

      if (percentageDefault) {
        targets.push({ target: percentageDefault, label: "default", parameterKey: "default" });
      }

      return targets;
    }

    case "parallel":
      return nodeRefArray(parameters.branches).map((target, parameterIndex) => ({
        target,
        label: "branch",
        parameterKey: "branches",
        parameterIndex,
      }));
    case "join":
      return nodeRefArray(parameters.wait_for).map((target, parameterIndex) => ({
        target,
        label: "wait_for",
        parameterKey: "wait_for",
        parameterIndex,
      }));

    case "try": {
      const targets: { target: string; label: string; parameterKey: string }[] = [];
      const body = nodeRefId(parameters.body);
      const catchTarget = nodeRefId(parameters.catch);
      const finallyTarget = nodeRefId(parameters.finally);

      if (body) {
        targets.push({ target: body, label: "body", parameterKey: "body" });
      }

      if (catchTarget) {
        targets.push({ target: catchTarget, label: "catch", parameterKey: "catch" });
      }

      if (finallyTarget) {
        targets.push({ target: finallyTarget, label: "finally", parameterKey: "finally" });
      }

      return targets;
    }

    case "map": {
      const target = nodeRefId(parameters.target);
      return target ? [{ target, label: "target", parameterKey: "target" }] : [];
    }

    case "race":
      return nodeRefArray(parameters.branches).map((target, parameterIndex) => ({
        target,
        label: "race",
        parameterKey: "branches",
        parameterIndex,
      }));
    default:
      return [];
  }
}

export function nodeRef(target: string): JsonRecord {
  return { $node: target };
}

export function nodeRefId(value: unknown): string | null {
  return isRecord(value) && typeof value.$node === "string" && value.$node.length > 0
    ? value.$node
    : null;
}

function nodeRefArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.map(nodeRefId).filter((item): item is string => Boolean(item))
    : [];
}

export function valueRef(
  source: "params" | "prev" | "workflow" | "config" | "input",
  path: (string | number)[],
): JsonRecord {
  return { $ref: { [source]: path } };
}

export function validateWorkflowReferenceSyntax(definition: JsonRecord): string[] {
  return validateWorkflowIssues(definition)
    .filter((issue) => issue.severity === "error")
    .map((issue) => issue.message);
}

function pushNodeRefIssue(
  issues: WorkflowValidationIssue[],
  nodeIds: Set<string>,
  nodeId: string,
  semanticKey: string,
  value: unknown,
  required: boolean,
) {
  const label = `${nodeId}.${semanticKey}`;

  if (value == null && !required) {
    return;
  }

  const target = nodeRefId(value);

  if (!target) {
    issues.push({
      severity: "error",
      nodeId,
      edgeKey: edgeValidationKey(nodeId, semanticKey),
      message: `${label} must be { "$node": "node_id" }`,
    });
    return;
  }

  if (!nodeIds.has(target)) {
    issues.push({
      severity: "error",
      nodeId,
      edgeKey: edgeValidationKey(nodeId, semanticKey),
      message: `${label} references missing node ${target}`,
    });
  }
}

function pushControlFlowIssues(
  issues: WorkflowValidationIssue[],
  node: JsonRecord,
  nodeIds: Set<string>,
  nodeId: string,
) {
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  const kind = workflowNodeKind(node.kind);

  if (kind === "switch") {
    const cases = recordArray(parameters.cases);
    cases.forEach((item: JsonRecord, index: number) => {
      pushNodeRefIssue(
        issues,
        nodeIds,
        nodeId,
        parameterSemanticKey("cases", index),
        item.target,
        true,
      );
    });
    pushNodeRefIssue(issues, nodeIds, nodeId, "default", parameters.default, false);
    return;
  }

  if (kind === "toggle") {
    pushNodeRefIssue(issues, nodeIds, nodeId, "on", parameters.on, true);
    pushNodeRefIssue(issues, nodeIds, nodeId, "off", parameters.off, true);
    return;
  }

  if (kind === "percentage") {
    const buckets = recordArray(parameters.buckets);
    buckets.forEach((item, index) => {
      pushNodeRefIssue(
        issues,
        nodeIds,
        nodeId,
        parameterSemanticKey("buckets", index),
        item.target,
        true,
      );
    });
    pushNodeRefIssue(issues, nodeIds, nodeId, "default", parameters.default, false);
    return;
  }

  if (kind === "parallel" || kind === "race") {
    const branches = asArray(parameters.branches);
    branches.forEach((target: unknown, index: number) => {
      pushNodeRefIssue(
        issues,
        nodeIds,
        nodeId,
        parameterSemanticKey("branches", index),
        target,
        true,
      );
    });
    return;
  }

  if (kind === "join") {
    const waitFor = asArray(parameters.wait_for);
    waitFor.forEach((target: unknown, index: number) => {
      pushNodeRefIssue(
        issues,
        nodeIds,
        nodeId,
        parameterSemanticKey("wait_for", index),
        target,
        true,
      );
    });
    return;
  }

  if (kind === "try") {
    for (const key of ["body", "catch", "finally"]) {
      pushNodeRefIssue(issues, nodeIds, nodeId, key, parameters[key], false);
    }

    return;
  }

  if (kind === "loop" || kind === "map") {
    pushNodeRefIssue(issues, nodeIds, nodeId, "target", parameters.target, false);
  }
}

function pushExpressionIssues(
  issues: WorkflowValidationIssue[],
  value: unknown,
  nodeIds: Set<string>,
  nodeId: string,
  label: string,
  edgeKey?: string,
) {
  if (value == null) {
    return;
  }

  if (typeof value === "string") {
    if (value.includes("{{") || value.includes("}}")) {
      issues.push({
        severity: "error",
        nodeId,
        edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
        message: `${label} uses removed template reference syntax`,
      });
    }

    return;
  }

  if (Array.isArray(value)) {
    value.forEach((item, index) => {
      pushExpressionIssues(issues, item, nodeIds, nodeId, `${label}[${String(index)}]`, edgeKey);
    });
    return;
  }

  if (!isRecord(value)) {
    return;
  }

  if ("$value" in value) {
    issues.push({
      severity: "error",
      nodeId,
      edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
      message: `${label} uses removed $value reference syntax`,
    });
  }

  const operators = ["$ref", "$concat", "$literal", "$node"].filter((key) => key in value);

  if (operators.length > 0 && Object.keys(value).length !== 1) {
    issues.push({
      severity: "error",
      nodeId,
      edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
      message: `${label} expression object must contain exactly one operator`,
    });
  }

  if (isRecord(value.$ref)) {
    if (typeof value.$ref.node === "string" && !nodeIds.has(value.$ref.node)) {
      issues.push({
        severity: "error",
        nodeId,
        edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
        message: `${label} references missing node ${value.$ref.node}`,
      });
    }

    if (value.$ref.input !== undefined) {
      issues.push({
        severity: "error",
        nodeId,
        edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
        message: `${label} uses removed input reference root`,
      });
    }

    for (const path of [
      value.$ref.params,
      value.$ref.prev,
      value.$ref.workflow,
      value.$ref.output,
    ]) {
      if (path !== undefined && !validRefPath(path)) {
        issues.push({
          severity: "error",
          nodeId,
          edgeKey: edgeKey ? edgeValidationKey(nodeId, edgeKey) : undefined,
          message: `${label} has invalid reference path`,
        });
      }
    }
  }

  if (Array.isArray(value.$concat)) {
    value.$concat.forEach((item, index) => {
      pushExpressionIssues(issues, item, nodeIds, nodeId, `${label}.$concat[${String(index)}]`, edgeKey);
    });
  }

  if (operators.length === 0) {
    Object.entries(value).forEach(([key, nested]) => {
      pushExpressionIssues(issues, nested, nodeIds, nodeId, `${label}.${key}`, edgeKey);
    });
  }
}

function pushProviderIssues(
  issues: WorkflowValidationIssue[],
  node: JsonRecord,
  providers: ProviderMetadata[],
  nodeId: string,
) {
  const kind = workflowNodeKind(node.kind);

  if (kind !== "action") {
    return;
  }

  const config = workflowNodeActionConfig(node);

  if (!config.provider || !config.action) {
    issues.push({
      severity: "warning",
      nodeId,
      message: `${nodeId} has no provider action selected`,
    });
    return;
  }

  if (providers.length === 0) {
    return;
  }

  const provider = providers.find((item) => item.name === config.provider);

  if (!provider) {
    issues.push({
      severity: "error",
      nodeId,
      message: `${nodeId} references unknown provider ${config.provider}`,
    });
    return;
  }

  const action = provider.actions.find((item) => item.function_name === config.action);

  if (!action) {
    issues.push({
      severity: "error",
      nodeId,
      message: `${nodeId} references unknown action ${config.provider}.${config.action}`,
    });
    return;
  }

  const inputs = workflowNodeActionInputs(node);

  for (const parameter of action.parameters) {
    if (!parameter.required) {
      continue;
    }

    if (isEmptyInputValue(inputs[parameter.name])) {
      issues.push({
        severity: "error",
        nodeId,
        message: `${nodeId}: ${parameter.label ?? parameter.name} is required`,
      });
    }
  }
}

function validRefPath(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.every((item) => typeof item === "string" || (Number.isInteger(item) && Number(item) >= 0))
  );
}

function renameNodeRefs(value: unknown, previousId: string, nextId: string) {
  if (Array.isArray(value)) {
    value.forEach((item) => {
      renameNodeRefs(item, previousId, nextId);
    });
    return;
  }

  if (!isRecord(value)) {
    return;
  }

  if (value.$node === previousId) {
    value.$node = nextId;
  }

  for (const nested of Object.values(value)) {
    renameNodeRefs(nested, previousId, nextId);
  }
}

function validationIssuesByNode(
  issues: WorkflowValidationIssue[],
): Map<string, WorkflowValidationIssue[]> {
  const map = new Map<string, WorkflowValidationIssue[]>();

  for (const issue of issues) {
    const list = map.get(issue.nodeId) ?? [];
    list.push(issue);
    map.set(issue.nodeId, list);
  }

  return map;
}

function validationIssuesByEdge(
  issues: WorkflowValidationIssue[],
): Map<string, WorkflowValidationIssue[]> {
  const map = new Map<string, WorkflowValidationIssue[]>();

  for (const issue of issues) {
    if (!issue.edgeKey) {
      continue;
    }

    const list = map.get(issue.edgeKey) ?? [];
    list.push(issue);
    map.set(issue.edgeKey, list);
  }

  return map;
}

function validationSeverity(
  issues: WorkflowValidationIssue[],
): WorkflowValidationSeverity | undefined {
  if (issues.some((issue) => issue.severity === "error")) {
    return "error";
  }

  return issues.length > 0 ? "warning" : undefined;
}

function edgeValidationKey(source: string, semanticKey: string): string {
  return `${source}:${semanticKey}`;
}

function semanticSourceHandleId(optionId: string): string {
  return `source:${optionId.replace(/:/g, ".")}`;
}

export function optionIdForSourceHandle(handleId?: string | null): string | null {
  return typeof handleId === "string" && handleId.startsWith("source:")
    ? handleId.slice("source:".length).replace(/\./g, ":")
    : null;
}

function workflowNodeKind(value: unknown): WorkflowNodeKind {
  return typeof value === "string" &&
    ["start", ...workflowNodeKinds, "loop", "end", "fail"].includes(value)
    ? (value as WorkflowNodeKind)
    : "action";
}

export function workflowNodeActionConfig(node: JsonRecord): { provider: string; action: string } {
  const action = isRecord(node.action) ? node.action : {};
  return {
    provider: displayValue(action.provider),
    action: displayValue(action.function),
  };
}

// effective action inputs, mirroring the runtime merge of action.configuration (base) with node.parameters (override).
export function workflowNodeActionInputs(node: JsonRecord): JsonRecord {
  const action = isRecord(node.action) ? node.action : null;
  const configuration = action && isRecord(action.configuration) ? action.configuration : {};
  const parameters = isRecord(node.parameters) ? node.parameters : {};
  return { ...configuration, ...parameters };
}

// a value sourced from another node/input counts as provided even before it resolves.
function isExpressionValue(value: unknown): boolean {
  if (!isRecord(value)) {
    return false;
  }

  return [
    "$ref",
    "$concat",
    "$coalesce",
    "$literal",
    "$to_string",
    "$to_json_string",
    "$node",
  ].some((key) => key in value);
}

function isEmptyInputValue(value: unknown): boolean {
  if (isExpressionValue(value)) {
    return false;
  }

  return isBlankValue(value);
}

export function workflowNodeResultMetadata(
  node: JsonRecord,
  providers: ProviderMetadata[],
): ActionResultMetadata[] {
  const config = workflowNodeActionConfig(node);

  if (!config.provider || !config.action) {
    return [];
  }

  const provider = providers.find((item) => item.name === config.provider);
  const action = provider?.actions.find((item) => item.function_name === config.action);
  return action?.results ?? [];
}

// the title shown on the node, falling back to the id when no custom name is set.
function nodeDisplayName(node: JsonRecord, id: string): string {
  const name = typeof node.name === "string" ? node.name.trim() : "";
  return name || id;
}

// renders any value (string, ref, expression, object) into a short human-readable label.
function describeValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }

  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  if (isRecord(value)) {
    if (typeof value.$node === "string") {
      return `→ ${value.$node}`;
    }

    if (isRecord(value.$ref)) {
      const [source, path] = Object.entries(value.$ref)[0] ?? [];
      const segments = Array.isArray(path) ? path.join(".") : "";
      return `\${${source}${segments ? `.${segments}` : ""}}`;
    }

    if ("$value" in value) {
      return describeValue(value.$value);
    }
  }

  if (Array.isArray(value)) {
    return value.length === 0 ? "[]" : `[${String(value.length)} item${value.length === 1 ? "" : "s"}]`;
  }

  try {
    const json = JSON.stringify(value);
    return json.length > 60 ? `${json.slice(0, 57)}…` : json;
  } catch {
    return "…";
  }
}

// each node kind renders a concise, never-"[object Object]" description of its activity.
function nodeSummary(node: JsonRecord, subflowNames?: Map<string, string>): string {
  const parameters = asRecord(node.parameters);

  switch (workflowNodeKind(node.kind)) {
    case "action": {
      const config = workflowNodeActionConfig(node);

      if (!config.provider) {
        return "Unconfigured action";
      }

      return config.action ? `${config.provider}.${config.action}` : config.provider;
    }

    case "approval":
      return describeValue(parameters.prompt) || "Approval required";

    case "condition": {
      const branches = asArray(asRecord(node.transitions).branches);
      const count = branches.length;
      return `${String(count)} branch${count === 1 ? "" : "es"}`;
    }

    case "switch": {
      const count = Array.isArray(parameters.cases) ? parameters.cases.length : 0;
      return `Switch on ${describeValue(parameters.value) || "value"} (${String(count)} case${count === 1 ? "" : "s"})`;
    }

    case "toggle":
      return `Toggle on ${describeValue(parameters.value) || "value"}`;

    case "percentage": {
      const count = Array.isArray(parameters.buckets) ? parameters.buckets.length : 0;
      return `Split on ${describeValue(parameters.key) || "key"} (${String(count)} bucket${count === 1 ? "" : "s"})`;
    }

    case "wait": {
      const wait = asRecord(node.wait);
      const until = wait.until_status;

      if (until) {
        return `Wait for ${describeValue(until)}`;
      }

      const seconds = Number(wait.seconds ?? 0);
      return seconds > 0 ? `Wait ${String(seconds)}s` : "Wait";
    }

    case "loop": {
      const target = nodeRefId(parameters.target);
      const max = Number(node.max_iterations ?? 0);
      return `Loop${target ? ` → ${target}` : ""}${max ? ` ×${String(max)}` : ""}`;
    }

    case "map": {
      const target = nodeRefId(parameters.target);
      const concurrency = Number(parameters.concurrency ?? 1);
      return `Map${target ? ` → ${target}` : ""} (×${String(concurrency)})`;
    }

    case "parallel": {
      const count = nodeRefArray(parameters.branches).length;
      return `${String(count)} parallel branch${count === 1 ? "" : "es"}`;
    }

    case "race": {
      const count = nodeRefArray(parameters.branches).length;
      return `Race ${String(count)} branch${count === 1 ? "" : "es"}`;
    }

    case "join": {
      const count = nodeRefArray(parameters.wait_for).length;
      return `Join ${String(count)} (${describeValue(parameters.mode) || "all"})`;
    }

    case "try": {
      const parts = ["body", "catch", "finally"].filter((key) => nodeRefId(parameters[key]));
      return parts.length ? `Try (${parts.join(", ")})` : "Try";
    }

    case "output":
      return `Output ${describeValue(parameters.event_type) || "workflow.output"}`;
    case "input":
      return describeValue(parameters.prompt) || "Input";
    case "config":
      return describeValue(parameters.name) || "Config";

    case "subflow": {
      const subflowId = node.subflow_id != null ? displayValue(node.subflow_id) : "";
      const name = subflowId ? subflowNames?.get(subflowId) : undefined;

      if (name) {
        return name;
      }

      return `Workflow ${subflowId || "-"}`;
    }

    case "start":
      return "Start";
    case "end":
      return "Success";
    case "fail":
      return "Workflow failure";
    default:
      return workflowNodeKind(node.kind);
  }
}

function approvalPrompt(node: JsonRecord, state?: JsonRecord): string | undefined {
  if (workflowNodeKind(node.kind) !== "approval") {
    return undefined;
  }

  return (
    describeValue(
      state?.prompt ?? asRecord(state?.approval).prompt ?? asRecord(node.parameters).prompt,
    ) || "Approval required"
  );
}

function inputPrompt(node: JsonRecord, state?: JsonRecord): string | undefined {
  if (workflowNodeKind(node.kind) !== "input") {
    return undefined;
  }

  return (
    describeValue(asRecord(state?.input).prompt ?? asRecord(node.parameters).prompt) ||
    "Input required"
  );
}

function firstAvailableTransition(node: JsonRecord): WorkflowDirectTransitionKey {
  const transitions = isRecord(node.transitions) ? node.transitions : {};
  return directTransitionKeys.find((key) => !transitions[key]) ?? "next";
}

function edgeHandles(
  definition: JsonRecord,
  source: string,
  semanticKey: string,
): Pick<
  WorkflowEditorEdgeData,
  "sourceHandle" | "targetHandle" | "edgeStyle" | "labelOffset" | "labelAnchor"
> {
  const edgeHandleMap = asRecord(asRecord(definition.ui).edge_handles);
  const saved = asRecord(edgeHandleMap[edgeHandleKey(source, semanticKey)]);
  return {
    sourceHandle:
      normalizeConnectionHandle(saved.sourceHandle) ??
      semanticSourceHandleId(optionIdFromSemanticKey(semanticKey)),
    targetHandle: normalizeConnectionHandle(saved.targetHandle) ?? semanticTargetHandleId,
    edgeStyle: normalizeWorkflowEdgeStyle(saved.edgeStyle),
    labelOffset: normalizeLabelOffset(saved.labelOffset),
    labelAnchor: normalizeLabelAnchor(saved.labelAnchor),
  };
}

function connectionHandlesForPositions(
  source?: WorkflowLayoutPosition,
  target?: WorkflowLayoutPosition,
): Pick<WorkflowEditorEdgeData, "sourceHandle" | "targetHandle"> {
  if (!source || !target) {
    return { sourceHandle: "bottom", targetHandle: "top" };
  }

  const dx = target.x - source.x;
  const dy = target.y - source.y;

  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx >= 0
      ? { sourceHandle: "right", targetHandle: "left" }
      : { sourceHandle: "left", targetHandle: "right" };
  }

  return dy >= 0
    ? { sourceHandle: "bottom", targetHandle: "top" }
    : { sourceHandle: "top", targetHandle: "bottom" };
}

export function parameterSemanticKey(parameterKey?: string, parameterIndex?: number): string {
  if (typeof parameterIndex === "number") {
    return `${parameterKey ?? "control"}.${String(parameterIndex)}`;
  }

  return parameterKey ?? "control";
}

function edgeHandleKey(source: string, semanticKey: string): string {
  return `${source}:${semanticKey}`;
}

function optionIdFromSemanticKey(semanticKey: string): string {
  if (directTransitionKeys.includes(semanticKey as WorkflowDirectTransitionKey)) {
    return `direct:${semanticKey}`;
  }

  if (semanticKey.startsWith("branches.")) {
    return `branch:${semanticKey.slice("branches.".length)}`;
  }

  if (semanticKey.includes(".")) {
    const [parameterKey, parameterIndex] = semanticKey.split(".");
    return `control:${parameterKey}:${parameterIndex}`;
  }

  return `control:${semanticKey}`;
}

function titleCase(value: string): string {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function transitionLabel(value: string): string {
  return titleCase(value.replace(/^on_/, ""));
}

function normalizeConnectionHandle(value: unknown): WorkflowConnectionHandle | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function normalizeWorkflowEdgeStyle(value: unknown): WorkflowEdgeStyle {
  return workflowEdgeStyles.includes(value as WorkflowEdgeStyle)
    ? (value as WorkflowEdgeStyle)
    : "square";
}

function normalizeLabelOffset(value: unknown): WorkflowEdgeLabelOffset | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const x = Number(value.x);
  const y = Number(value.y);

  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return undefined;
  }

  if (x === 0 && y === 0) {
    return undefined;
  }

  return { x, y };
}

function normalizeLabelAnchor(value: unknown): WorkflowEdgeLabelAnchor | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const position = Number(value.position);

  if (!Number.isFinite(position)) {
    return undefined;
  }

  const clamped = Math.min(Math.max(position, 0), 1);

  if (Math.abs(clamped - 0.5) < 0.001) {
    return undefined;
  }

  return { position: clamped };
}
