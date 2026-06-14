<template>
  <div class="panel workflow-canvas-panel" @pointermove="trackPointer">
    <WorkflowToolbar />
    <SplitPane
      class="workflow-editor-split"
      orientation="horizontal"
      storage-key="command-center.workflows.editor-split"
      :initial-first-pct="58"
      :min-first="360"
      :min-second="360"
    >
      <template #first>
        <div class="workflow-graph-pane">
          <VueFlow
            class="workflow-graph"
            :nodes="workflows.graphNodes"
            :edges="workflows.graphEdges"
            @node-click="workflows.onGraphNodeClick"
            @node-double-click="workflows.onGraphNodeDoubleClick"
            @node-context-menu="openNodeMenu"
            @node-drag-stop="workflows.onGraphNodeDragStop"
            @nodes-change="workflows.onGraphNodesChange"
            @connect="openConnectMenu"
            @edge-update="workflows.onGraphEdgeUpdate"
            @edge-click="openEdgeEditorFromEvent"
            @edge-context-menu="openEdgeMenu"
            @edge-double-click="openEdgeEditorFromEvent"
            @edges-change="workflows.onGraphEdgesChange"
            @pane-click="closeOverlaysAndSelection"
            @pane-context-menu="closeOverlaysAndSelection"
            :edges-updatable="true"
            delete-key-code="Delete"
            :snap-to-grid="true"
            :snap-grid="[15, 15]"
          >
            <template #node-workflow="nodeProps">
              <WorkflowNode v-bind="nodeProps" />
            </template>
            <template #edge-workflow="edgeProps">
              <WorkflowEdge v-bind="edgeProps" />
            </template>
          </VueFlow>
          <div class="workflow-issues-panel">
            <header class="workflow-issues-header">
              <span>Diagnostics</span>
              <span :class="['workflow-issues-summary', issueSummaryClass]">{{ issueSummary }}</span>
            </header>
            <table v-if="issueRows.length" class="workflow-issues-table">
              <thead>
                <tr>
                  <th>Type</th>
                  <th>What</th>
                  <th>Node</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(row, index) in issueRows" :key="index" :class="row.severity" @click="focusIssueNode(row.nodeId)">
                  <td><span :class="['workflow-issue-severity', row.severity]">{{ row.severity }}</span></td>
                  <td>{{ row.message }}</td>
                  <td class="workflow-issue-node-cell">{{ row.title }}</td>
                </tr>
              </tbody>
            </table>
            <div v-else class="workflow-issues-empty">No graph diagnostics.</div>
          </div>
        </div>
      </template>
      <template #second>
        <div class="workflow-wdl-pane">
          <p v-if="workflows.workflowWdlError" class="workflow-wdl-error">
            This workflow can't be shown as WDL yet ({{ workflows.workflowWdlError }}). Edit it in the graph until it is well-formed.
          </p>
          <WdlEditor
            v-model="workflows.workflowWdl"
            class="workflow-wdl-editor"
            :readonly="Boolean(workflows.workflowWdlError)"
            :providers="providersStore.providers"
            :settings="secretsStore.secrets"
          />
        </div>
      </template>
    </SplitPane>
    <div v-if="showCommandBar" class="workflow-command-bar">
      <template v-if="workflows.selectedGraphEdge">
        <button @click="editSelectedEdge">Edit</button>
        <button @click="workflows.removeWorkflowEdgeById(workflows.selectedGraphEdge.id)">Delete</button>
        <button @click="workflows.reverseSelectedEdgeHandles">Reverse handles</button>
        <button :disabled="!selectedEdgeCanMoveUp" @click="workflows.moveSelectedEdge(-1)">Move up</button>
        <button :disabled="!selectedEdgeCanMoveDown" @click="workflows.moveSelectedEdge(1)">Move down</button>
        <span v-if="workflows.selectedEdgeIssues.length" class="workflow-command-issues">{{ workflows.selectedEdgeIssues[0].message }}</span>
      </template>
      <template v-else-if="workflows.selectedNode">
        <button @click="workflows.openStepEditor(workflows.selectedStepId)">Edit</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.duplicateSelectedStep">Duplicate</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Delete</button>
        <button @click="workflows.addConnectedWorkflowNode('action')">Add node</button>
        <button @click="workflows.autoArrangeWorkflowNodes()">Auto arrange from here</button>
        <span v-if="workflows.selectedNodeIssues.length" class="workflow-command-issues">{{ workflows.selectedNodeIssues[0].message }}</span>
      </template>
    </div>
    <div
      v-if="contextMenu"
      class="workflow-context-menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      @pointerdown.stop
      @mousedown.stop
      @click.stop
      @contextmenu.prevent
    >
      <button v-if="contextMenu.kind === 'node'" :disabled="!contextMenu.deletable" @click="deleteContextNode">Delete node</button>
      <button v-if="contextMenu.kind === 'edge'" @click="editContextEdge">Edit edge</button>
      <button v-if="contextMenu.kind === 'edge'" @click="deleteContextEdge">Delete edge</button>
    </div>
    <div
      v-if="pendingConnect"
      class="workflow-edge-popover"
      :style="{ left: `${pendingConnect.x}px`, top: `${pendingConnect.y}px` }"
      @pointerdown.stop
      @mousedown.stop
      @click.stop
      @contextmenu.prevent
    >
      <strong>Edge type</strong>
      <button v-for="option in pendingConnect.options" :key="option.id" @click="applyPendingConnect(option.id)">
        <span>{{ option.label }}</span>
        <small>{{ option.description }}</small>
      </button>
    </div>
    <form
      v-if="edgeEditor"
      class="workflow-edge-popover workflow-edge-editor"
      :style="{ left: `${edgeEditor.x}px`, top: `${edgeEditor.y}px` }"
      @pointerdown.stop
      @mousedown.stop
      @submit.prevent="applyEdgeEditor"
      @click.stop
      @contextmenu.prevent
    >
      <strong>Edit edge</strong>
      <label>
        Type
        <select v-model="edgeEditor.optionId">
          <option v-for="option in edgeEditorOptions" :key="option.id" :value="option.id">{{ option.label }}</option>
        </select>
      </label>
      <label>
        Target
        <select v-model="edgeEditor.target">
          <option v-for="nodeId in workflowNodeIds" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
        </select>
      </label>
      <label>
        Edge style
        <select v-model="edgeEditor.edgeStyle">
          <option v-for="option in edgeStyleOptions" :key="option.value" :value="option.value">{{ option.label }}</option>
        </select>
      </label>
      <label class="workflow-edge-anchor-field">
        Label anchor
        <div class="workflow-edge-anchor-control">
          <input v-model.number="edgeEditor.labelAnchor" type="range" min="0" max="100" step="5" />
          <span>{{ Math.round(edgeEditor.labelAnchor) }}%</span>
        </div>
      </label>
      <label v-if="edgeEditorCanEditLabel">
        Label
        <input v-model="edgeEditor.label" placeholder="Uses default label when empty" />
      </label>
      <label v-if="edgeEditorIsConditionBranch">
        When
        <ExpressionJsonEditor v-model="edgeEditor.whenJson" :context="edgeExpressionContext" class="workflow-edge-json" title="When" />
      </label>
      <template v-if="edgeEditorIsSwitchCase">
        <label>
          Match
          <select v-model="edgeEditor.matchKind">
            <option value="equals">equals</option>
            <option value="not_equals">not_equals</option>
            <option value="exists">exists</option>
            <option value="when">when</option>
          </select>
        </label>
        <label>
          Match
          <ExpressionJsonEditor v-model="edgeEditor.matchJson" :context="edgeExpressionContext" class="workflow-edge-json" title="Match" />
        </label>
      </template>
      <div v-if="edgeEditorCanMove" class="workflow-edge-editor-actions">
        <button type="button" :disabled="edgeEditor.orderIndex <= 0" @click="moveEdgeEditor(-1)">Move up</button>
        <button type="button" :disabled="edgeEditor.orderIndex >= edgeEditor.orderCount - 1" @click="moveEdgeEditor(1)">Move down</button>
      </div>
      <div class="workflow-edge-editor-actions">
        <button type="submit">Apply</button>
        <button type="button" @click="closeEdgeEditor">Cancel</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { watch, nextTick, ref, computed, provide } from "vue";
import { VueFlow, useVueFlow } from "@vue-flow/core";
import type { WorkflowEdgeEditorDraft, WorkflowEdgeSemanticOption } from "../../types/models";
import { useWorkflowsStore } from "../../stores/workflows";
import { useProvidersStore } from "../../stores/providers";
import { useSecretsStore } from "../../stores/secrets";
import { optionIdForSourceHandle } from "../../utils/workflows";
import { buildSampleContext } from "../../utils/workflow-references";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
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
const { fitView, flowToScreenCoordinate, onPaneReady } = useVueFlow();
const contextMenu = ref<null | { kind: "node"; id: string; x: number; y: number; deletable: boolean } | { kind: "edge"; id: string; x: number; y: number }>(null);
const lastPointer = ref({ x: 0, y: 0 });
const pendingConnect = ref<null | { connection: any; x: number; y: number; options: WorkflowEdgeSemanticOption[] }>(null);
const edgeEditor = ref<null | (WorkflowEdgeEditorDraft & { x: number; y: number })>(null);
const nodeWidth = 180;
const nodeHeight = 64;
const popoverMargin = 12;
const edgeEditorWidth = 340;
const edgeEditorMinVisibleHeight = 260;
const edgeStyleOptions = [
  { value: "bezier", label: "Bezier" },
  { value: "straight", label: "Straight" },
  { value: "square", label: "Square" }
];
const workflowNodeIds = computed(() => {
  const nodes = workflows.workflowDraft.definition?.nodes;
  return Array.isArray(nodes) ? nodes.map((node: any) => String(node.id ?? "")).filter(Boolean) : [];
});
// references in scope for the edge's condition/match expressions, anchored at the edge source node.
const edgeExpressionContext = computed(() => ({
  workflowInputType: workflows.workflowDraft.input_type ?? null,
  nodes: workflows.workflowDraft.definition?.nodes ?? [],
  currentNodeId: edgeEditor.value?.source ?? "",
  providers: providersStore.providers,
  sampleContext: buildSampleContext(workflows.workflowRunDetail)
}));
const edgeEditorOptions = computed(() => edgeEditor.value ? workflows.workflowEdgeOptions(edgeEditor.value.source) : []);
const edgeEditorIsConditionBranch = computed(() => edgeEditor.value?.optionId.startsWith("branch:") ?? false);
const edgeEditorIsSwitchCase = computed(() => edgeEditor.value?.optionId.startsWith("control:cases:") ?? false);
const edgeEditorCanEditLabel = computed(() => edgeEditorIsConditionBranch.value || edgeEditorIsSwitchCase.value);
const edgeEditorCanMove = computed(() => {
  const optionId = edgeEditor.value?.optionId ?? "";
  return Boolean(edgeEditor.value?.canMove) && !optionId.endsWith(":new") && (
    optionId.startsWith("branch:") ||
    optionId.startsWith("control:cases:") ||
    optionId.startsWith("control:branches:") ||
    optionId.startsWith("control:wait_for:")
  );
});
const selectedEdgeDraft = computed(() => workflows.selectedGraphEdgeId ? workflows.openEdgeEditorDraft(workflows.selectedGraphEdgeId) : null);
const selectedEdgeCanMoveUp = computed(() => Boolean(selectedEdgeDraft.value?.canMove && selectedEdgeDraft.value.orderIndex > 0));
const selectedEdgeCanMoveDown = computed(() => Boolean(selectedEdgeDraft.value?.canMove && selectedEdgeDraft.value.orderIndex < selectedEdgeDraft.value.orderCount - 1));
const showCommandBar = computed(() => Boolean(workflows.selectedGraphEdge || workflows.selectedNode));

// group validation issues by node so misconfigured nodes can be listed under the graph.
// flatten validation issues into table rows, errors first, mirroring the wdl editor diagnostics.
const issueRows = computed(() => {
  const titles = new Map(workflows.graphNodes.map((node: any) => [node.id, String(node.data?.title ?? node.id)]));
  return [...workflows.graphValidationIssues]
    .map((issue) => ({
      severity: issue.severity,
      message: issue.message,
      nodeId: issue.nodeId,
      title: titles.get(issue.nodeId) ?? issue.nodeId
    }))
    .sort((left, right) => Number(right.severity === "error") - Number(left.severity === "error"));
});

const issueCounts = computed(() => {
  const errors = workflows.graphValidationIssues.filter((issue) => issue.severity === "error").length;
  return { errors, warnings: workflows.graphValidationIssues.length - errors };
});

const issueSummary = computed(() => {
  const { errors, warnings } = issueCounts.value;
  const parts: string[] = [];
  if (errors) parts.push(`${errors} error${errors === 1 ? "" : "s"}`);
  if (warnings) parts.push(`${warnings} warning${warnings === 1 ? "" : "s"}`);
  return parts.join(" · ") || "Clean";
});

const issueSummaryClass = computed(() => {
  if (issueCounts.value.errors) return "error";
  if (issueCounts.value.warnings) return "warning";
  return "clean";
});

// select the node and recenter the graph on it so the user can fix it.
function focusIssueNode(nodeId: string) {
  workflows.populateStepEditor(nodeId);
  nextTick(() => fitView({ nodes: [nodeId], duration: 400, maxZoom: 1.2 }));
}

async function recenter() {
  await nextTick();
  fitView();
}

onPaneReady(() => {
  recenter();
});

watch(() => workflows.selectedWorkflowId, () => {
  recenter();
});

watch(() => workflows.workflowLayoutVersion, () => {
  recenter();
});

function openNodeMenu(event: any) {
  const mouse = event?.event as MouseEvent | undefined;
  const node = event?.node;
  if (!mouse || !node?.id) return;
  mouse.preventDefault();
  mouse.stopPropagation();
  contextMenu.value = {
    kind: "node",
    id: node.id,
    x: mouse.clientX,
    y: mouse.clientY,
    deletable: node.data?.locked !== true
  };
}

function openEdgeMenu(event: any) {
  const mouse = event?.event as MouseEvent | undefined;
  const edge = event?.edge;
  if (!mouse || !edge?.id) return;
  mouse.preventDefault();
  mouse.stopPropagation();
  contextMenu.value = { kind: "edge", id: edge.id, x: mouse.clientX, y: mouse.clientY };
}

function openEdgeEditorFromEvent(event: any) {
  const mouse = event?.event as MouseEvent | undefined;
  const edge = event?.edge;
  if (!edge?.id) return;
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

function openConnectMenu(connection: any) {
  const source = connection?.source;
  const options = source ? workflows.workflowEdgeOptions(source) : [];
  if (!source || !connection?.target || options.length === 0) return;
  const handleOptionId = optionIdForSourceHandle(connection?.sourceHandle);
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
    y: lastPointer.value.y || window.innerHeight / 2
  };
}

function editSelectedEdge() {
  if (!workflows.selectedGraphEdge) return;
  openEdgeEditorForEdge(workflows.selectedGraphEdge.id);
}

function applyPendingConnect(optionId: string) {
  if (!pendingConnect.value) return;
  workflows.applyGraphEdgeSemantic(pendingConnect.value.connection, optionId);
  pendingConnect.value = null;
}

function deleteContextNode() {
  if (contextMenu.value?.kind !== "node" || !contextMenu.value.deletable) return;
  workflows.removeWorkflowNode(contextMenu.value.id);
  closeContextMenu();
}

function deleteContextEdge() {
  if (contextMenu.value?.kind !== "edge") return;
  workflows.removeWorkflowEdgeById(contextMenu.value.id);
  closeContextMenu();
}

function editContextEdge() {
  if (contextMenu.value?.kind !== "edge") return;
  const menu = contextMenu.value;
  workflows.selectGraphEdge(menu.id);
  openEdgeEditorForEdge(menu.id, { x: menu.x, y: menu.y });
}

function openEdgeEditorAt(edgeId: string, x: number, y: number) {
  const draft = workflows.openEdgeEditorDraft(edgeId);
  if (!draft) return;
  const position = clampEdgeEditorPosition(x, y);
  edgeEditor.value = {
    ...draft,
    x: position.x,
    y: position.y
  };
  closeContextMenu();
}

function openEdgeEditorForEdge(edgeId: string, fallback = lastPointer.value) {
  const position = edgeEditorPosition(edgeId, fallback);
  openEdgeEditorAt(edgeId, position.x, position.y);
}

function edgeEditorPosition(edgeId: string, fallback: { x: number; y: number }) {
  const edge = workflows.graphEdges.find((item: any) => item.id === edgeId);
  const source = edge ? workflows.graphNodes.find((item: any) => item.id === edge.source) : null;
  const target = edge ? workflows.graphNodes.find((item: any) => item.id === edge.target) : null;
  if (!edge || !source || !target) return clampEdgeEditorPosition(fallback.x, fallback.y);
  const midpoint = {
    x: (Number(source.position?.x ?? 0) + Number(target.position?.x ?? 0)) / 2 + nodeWidth / 2,
    y: (Number(source.position?.y ?? 0) + Number(target.position?.y ?? 0)) / 2 + nodeHeight / 2
  };
  const screenPoint = flowToScreenCoordinate(midpoint);
  return clampEdgeEditorPosition(screenPoint.x + 16, screenPoint.y - 16);
}

function clampEdgeEditorPosition(x: number, y: number) {
  const maxX = Math.max(popoverMargin, window.innerWidth - edgeEditorWidth - popoverMargin);
  const maxY = Math.max(popoverMargin, window.innerHeight - edgeEditorMinVisibleHeight);
  return {
    x: Math.min(Math.max(popoverMargin, x), maxX),
    y: Math.min(Math.max(popoverMargin, y), maxY)
  };
}

function applyEdgeEditor() {
  if (!edgeEditor.value) return;
  if (workflows.applyEdgeEditorDraft(edgeEditor.value)) closeEdgeEditor();
}

function closeEdgeEditor() {
  edgeEditor.value = null;
}

function moveEdgeEditor(direction: -1 | 1) {
  if (!edgeEditor.value) return;
  const moved = workflows.moveEdgeEditorItem(edgeEditor.value, direction);
  if (!moved) return;
  edgeEditor.value = {
    ...moved,
    x: edgeEditor.value.x,
    y: edgeEditor.value.y
  };
}
</script>
