<template src="./workflow-canvas.template.html"></template>

<script setup lang="ts">
/* eslint-disable @typescript-eslint/no-unused-vars -- external Vue templates are not visible to ESLint. */
import { watch, nextTick, ref, computed, provide } from "vue";
import {
  VueFlow,
  useVueFlow,
  type Connection,
  type EdgeMouseEvent,
  type NodeMouseEvent,
} from "@vue-flow/core";
import type {
  JsonRecord,
  WorkflowEdgeEditorDraft,
  WorkflowEdgeSemanticOption,
} from "../../../core/domain/models";
import { workflowInputType } from "../../../core/domain/models";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useProvidersStore } from "../../../ui/adapters/pinia/providers";
import { useSecretsStore } from "../../../ui/adapters/pinia/secrets";
import { useCatalogMetadataStore } from "../../../ui/adapters/pinia/catalogMetadata";
import { optionIdForSourceHandle } from "../../../core/workflow";
import { jsonRecordArray as recordArray } from "../../../core/domain/json";
import { HEADER_ISSUE_NODE_ID } from "../../../core/workflow/header-validation";
import { buildSampleContext } from "../../../core/utils/workflow-references";
import { displayValue } from "../../../core/utils/values";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import Icon from "../shared/Icon.vue";
import SplitPane from "../shared/SplitPane.vue";
import WdlEditor from "../shared/WdlEditor.vue";
import WorkflowToolbar from "./WorkflowToolbar.vue";
import WorkflowNode from "./WorkflowNode.vue";
import WorkflowEdge from "./WorkflowEdge.vue";

// edges in the editable canvas allow manual label repositioning.
provide("workflowEdgeInteractive", true);

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const secretsStore = useSecretsStore();
const catalogMetadata = useCatalogMetadataStore();
const { fitView, flowToScreenCoordinate, onPaneReady } = useVueFlow();
const contextMenu = ref<
  | null
  | { kind: "node"; id: string; x: number; y: number; deletable: boolean }
  | { kind: "edge"; id: string; x: number; y: number }
>(null);
const lastPointer = ref({ x: 0, y: 0 });
const pendingConnect = ref<null | {
  connection: Connection;
  x: number;
  y: number;
  options: WorkflowEdgeSemanticOption[];
}>(null);
const edgeEditor = ref<null | (WorkflowEdgeEditorDraft & { x: number; y: number })>(null);
const nodeWidth = 180;
const nodeHeight = 64;
const popoverMargin = 12;
const edgeEditorWidth = 340;
const edgeEditorMinVisibleHeight = 260;
const edgeStyleOptions = [
  { value: "bezier", label: "Bezier" },
  { value: "straight", label: "Straight" },
  { value: "square", label: "Square" },
];
const workflowNodeIds = computed(() => {
  return recordArray(workflows.workflowDraft.definition.nodes)
    .map((node) => displayValue(node.id))
    .filter(Boolean);
});
// references in scope for the edge's condition/match expressions, anchored at the edge source node.
const edgeExpressionContext = computed(() => ({
  workflowInputType: workflowInputType(workflows.workflowDraft),
  nodes: recordArray(workflows.workflowDraft.definition.nodes),
  currentNodeId: edgeEditor.value?.source ?? "",
  providers: providersStore.providers,
  sampleContext: buildSampleContext(workflows.workflowRunDetail),
}));
const edgeEditorOptions = computed(() =>
  edgeEditor.value ? workflows.workflowEdgeOptions(edgeEditor.value.source) : [],
);
const edgeEditorIsConditionBranch = computed(
  () => edgeEditor.value?.optionId.startsWith("branch:") ?? false,
);
const edgeEditorIsSwitchCase = computed(
  () => edgeEditor.value?.optionId.startsWith("control:cases:") ?? false,
);
const edgeEditorCanEditLabel = computed(
  () => edgeEditorIsConditionBranch.value || edgeEditorIsSwitchCase.value,
);
// bridge the numeric input to the draft's nullable priority; a blank/invalid entry clears it.
const edgeEditorPriority = computed<number | null>({
  get: () => edgeEditor.value?.priority ?? null,
  set: (value) => {
    if (!edgeEditor.value) {
      return;
    }

    edgeEditor.value.priority =
      typeof value === "number" && Number.isFinite(value) ? Math.trunc(value) : null;
  },
});
// match_kind options driven from catalog only.
const matchKindOptions = computed(() => catalogMetadata.enumOptions("match_kind"));
const matchKindsLoaded = computed(() => matchKindOptions.value.length > 0);

const edgeEditorCanMove = computed(() => {
  const optionId = edgeEditor.value?.optionId ?? "";
  return (
    Boolean(edgeEditor.value?.canMove) &&
    !optionId.endsWith(":new") &&
    (optionId.startsWith("branch:") ||
      optionId.startsWith("control:cases:") ||
      optionId.startsWith("control:branches:") ||
      optionId.startsWith("control:wait_for:"))
  );
});
const selectedEdgeDraft = computed(() =>
  workflows.selectedGraphEdgeId
    ? workflows.openEdgeEditorDraft(workflows.selectedGraphEdgeId)
    : null,
);
const selectedEdgeCanMoveUp = computed(() =>
  Boolean(selectedEdgeDraft.value?.canMove && selectedEdgeDraft.value.orderIndex > 0),
);
const selectedEdgeCanMoveDown = computed(() =>
  Boolean(
    selectedEdgeDraft.value?.canMove &&
    selectedEdgeDraft.value.orderIndex < selectedEdgeDraft.value.orderCount - 1,
  ),
);
const showCommandBar = computed(() =>
  Boolean(workflows.selectedGraphEdge ?? workflows.selectedNode),
);

// group validation issues by node so misconfigured nodes can be listed under the graph.
// flatten validation issues into table rows, errors first, mirroring the wdl editor diagnostics.
const issueRows = computed(() => {
  const titles = new Map(
    workflows.graphNodes.map((node) => [
      node.id,
      displayValue((node.data as JsonRecord | undefined)?.title ?? node.id),
    ]),
  );
  return [...workflows.graphValidationIssues]
    .map((issue) => ({
      severity: issue.severity,
      message: issue.message,
      nodeId: issue.nodeId,
      title: titles.get(issue.nodeId) ?? issue.nodeId,
    }))
    .sort((left, right) => Number(right.severity === "error") - Number(left.severity === "error"));
});

const issueCounts = computed(() => {
  const errors = workflows.graphValidationIssues.filter(
    (issue) => issue.severity === "error",
  ).length;
  return { errors, warnings: workflows.graphValidationIssues.length - errors };
});

const issueSummary = computed(() => {
  const { errors, warnings } = issueCounts.value;
  const parts: string[] = [];

  if (errors) {
    parts.push(`${String(errors)} error${errors === 1 ? "" : "s"}`);
  }

  if (warnings) {
    parts.push(`${String(warnings)} warning${warnings === 1 ? "" : "s"}`);
  }

  return parts.join(" · ") || "Clean";
});

const issueSummaryClass = computed(() => {
  if (issueCounts.value.errors) {
    return "error";
  }

  if (issueCounts.value.warnings) {
    return "warning";
  }

  return "clean";
});

// select the node and recenter the graph on it so the user can fix it.
function focusIssueNode(nodeId: string) {
  // a header issue belongs to the workflow, not to a node; there is nothing on the canvas to focus,
  // and populating the step editor with this id would select a node that does not exist.
  if (nodeId === HEADER_ISSUE_NODE_ID) {
    workflows.openWorkflowHeader();
    return;
  }

  workflows.populateStepEditor(nodeId);
  void nextTick(() => fitView({ nodes: [nodeId], duration: 400, maxZoom: 1.2 }));
}

async function recenter() {
  await nextTick();
  void fitView();
}

onPaneReady(() => {
  void recenter();
});

watch(
  () => workflows.selectedWorkflowId,
  () => {
    void recenter();
  },
);

watch(
  () => workflows.workflowLayoutVersion,
  () => {
    void recenter();
  },
);

function openNodeMenu(event: NodeMouseEvent) {
  const mouse = event.event as MouseEvent | undefined;
  const node = event.node;

  if (!mouse || !node.id) {
    return;
  }

  mouse.preventDefault();
  mouse.stopPropagation();
  contextMenu.value = {
    kind: "node",
    id: node.id,
    x: mouse.clientX,
    y: mouse.clientY,
    deletable: (node.data as JsonRecord | undefined)?.locked !== true,
  };
}

function openEdgeMenu(event: EdgeMouseEvent) {
  const mouse = event.event as MouseEvent | undefined;
  const edge = event.edge;

  if (!mouse || !edge.id) {
    return;
  }

  mouse.preventDefault();
  mouse.stopPropagation();
  contextMenu.value = { kind: "edge", id: edge.id, x: mouse.clientX, y: mouse.clientY };
}

function openEdgeEditorFromEvent(event: EdgeMouseEvent) {
  const mouse = event.event as MouseEvent | undefined;
  const edge = event.edge;

  if (!edge.id) {
    return;
  }

  mouse?.preventDefault();
  mouse?.stopPropagation();
  workflows.selectGraphEdge(edge.id);
  openEdgeEditorForEdge(edge.id, mouse ? { x: mouse.clientX, y: mouse.clientY } : undefined);
}

function closeContextMenu() {
  contextMenu.value = null;
}

function closeOverlays() {
  contextMenu.value = null;
  pendingConnect.value = null;
  edgeEditor.value = null;
}

function closeOverlaysAndSelection() {
  closeOverlays();
  workflows.clearWorkflowGraphSelection();
}

function trackPointer(event: PointerEvent) {
  lastPointer.value = { x: event.clientX, y: event.clientY };
}

function openConnectMenu(connection: Connection) {
  const source = connection.source;
  const options = source ? workflows.workflowEdgeOptions(source) : [];

  if (!source || !connection.target || options.length === 0) {
    return;
  }

  const handleOptionId = optionIdForSourceHandle(connection.sourceHandle);

  if (handleOptionId && options.some((option) => option.id === handleOptionId)) {
    workflows.applyGraphEdgeSemantic(connection, handleOptionId);
    return;
  }

  if (options.length === 1) {
    workflows.applyGraphEdgeSemantic(connection, options[0].id);
    return;
  }

  closeContextMenu();
  pendingConnect.value = {
    connection,
    options,
    x: lastPointer.value.x || window.innerWidth / 2,
    y: lastPointer.value.y || window.innerHeight / 2,
  };
}

function editSelectedEdge() {
  if (!workflows.selectedGraphEdge) {
    return;
  }

  openEdgeEditorForEdge(workflows.selectedGraphEdge.id);
}

function applyPendingConnect(optionId: string) {
  if (!pendingConnect.value) {
    return;
  }

  workflows.applyGraphEdgeSemantic(pendingConnect.value.connection, optionId);
  pendingConnect.value = null;
}

function deleteContextNode() {
  if (contextMenu.value?.kind !== "node" || !contextMenu.value.deletable) {
    return;
  }

  workflows.removeWorkflowNode(contextMenu.value.id);
  closeContextMenu();
}

function deleteContextEdge() {
  if (contextMenu.value?.kind !== "edge") {
    return;
  }

  workflows.removeWorkflowEdgeById(contextMenu.value.id);
  closeContextMenu();
}

function editContextEdge() {
  if (contextMenu.value?.kind !== "edge") {
    return;
  }

  const menu = contextMenu.value;
  workflows.selectGraphEdge(menu.id);
  openEdgeEditorForEdge(menu.id, { x: menu.x, y: menu.y });
}

function openEdgeEditorAt(edgeId: string, x: number, y: number) {
  const draft = workflows.openEdgeEditorDraft(edgeId);

  if (!draft) {
    return;
  }

  const position = clampEdgeEditorPosition(x, y);
  edgeEditor.value = {
    ...draft,
    x: position.x,
    y: position.y,
  };
  closeContextMenu();
}

function openEdgeEditorForEdge(edgeId: string, fallback = lastPointer.value) {
  const position = edgeEditorPosition(edgeId, fallback);
  openEdgeEditorAt(edgeId, position.x, position.y);
}

function edgeEditorPosition(edgeId: string, fallback: { x: number; y: number }) {
  const edge = workflows.graphEdges.find((item) => item.id === edgeId);
  const source = edge ? workflows.graphNodes.find((item) => item.id === edge.source) : null;
  const target = edge ? workflows.graphNodes.find((item) => item.id === edge.target) : null;

  if (!edge || !source || !target) {
    return clampEdgeEditorPosition(fallback.x, fallback.y);
  }

  const midpoint = {
    x: (source.position.x + target.position.x) / 2 + nodeWidth / 2,
    y: (source.position.y + target.position.y) / 2 + nodeHeight / 2,
  };
  const screenPoint = flowToScreenCoordinate(midpoint);
  return clampEdgeEditorPosition(screenPoint.x + 16, screenPoint.y - 16);
}

function clampEdgeEditorPosition(x: number, y: number) {
  const maxX = Math.max(popoverMargin, window.innerWidth - edgeEditorWidth - popoverMargin);
  const maxY = Math.max(popoverMargin, window.innerHeight - edgeEditorMinVisibleHeight);
  return {
    x: Math.min(Math.max(popoverMargin, x), maxX),
    y: Math.min(Math.max(popoverMargin, y), maxY),
  };
}

function applyEdgeEditor() {
  if (!edgeEditor.value) {
    return;
  }

  if (workflows.applyEdgeEditorDraft(edgeEditor.value)) {
    closeEdgeEditor();
  }
}

function closeEdgeEditor() {
  edgeEditor.value = null;
}

function moveEdgeEditor(direction: -1 | 1) {
  if (!edgeEditor.value) {
    return;
  }

  const moved = workflows.moveEdgeEditorItem(edgeEditor.value, direction);

  if (!moved) {
    return;
  }

  edgeEditor.value = {
    ...moved,
    x: edgeEditor.value.x,
    y: edgeEditor.value.y,
  };
}
</script>
