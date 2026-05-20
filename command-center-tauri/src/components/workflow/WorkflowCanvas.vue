<template>
  <div class="panel workflow-canvas-panel" @pointermove="trackPointer">
    <WorkflowToolbar />
    <div class="workflow-mode-tabs">
      <button :class="{ active: workflows.workflowEditorMode === 'graph' }" @click="workflows.workflowEditorMode = 'graph'">Graph</button>
      <button :class="{ active: workflows.workflowEditorMode === 'json' }" @click="workflows.workflowEditorMode = 'json'">JSON</button>
    </div>
    <VueFlow
      v-show="workflows.workflowEditorMode === 'graph'"
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
      @edge-context-menu="openEdgeMenu"
      @edge-double-click="openEdgeEditorFromEvent"
      @edges-change="workflows.onGraphEdgesChange"
      @pane-click="closeOverlays"
      @pane-context-menu="closeOverlays"
      :edges-updatable="true"
      delete-key-code="Delete"
      :snap-to-grid="true"
      :snap-grid="[15, 15]"
    >
      <template #node-workflow="nodeProps">
        <WorkflowNode v-bind="nodeProps" />
      </template>
    </VueFlow>
    <div
      v-if="contextMenu"
      class="workflow-context-menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
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
      <label v-if="edgeEditorCanEditLabel">
        Label
        <input v-model="edgeEditor.label" placeholder="Uses default label when empty" />
      </label>
      <label v-if="edgeEditorIsConditionBranch">
        When JSON
        <JsonEditor v-model="edgeEditor.whenJson" class="workflow-edge-json" />
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
          Match JSON
          <JsonEditor v-model="edgeEditor.matchJson" class="workflow-edge-json" />
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
    <JsonEditor
      v-show="workflows.workflowEditorMode === 'json'"
      v-model="workflows.workflowJson"
      class="workflow-json-editor"
      @blur="workflows.syncWorkflowJson"
    />
  </div>
</template>

<script setup lang="ts">
import { watch, nextTick, ref, computed } from "vue";
import { VueFlow, useVueFlow } from "@vue-flow/core";
import type { WorkflowEdgeEditorDraft, WorkflowEdgeSemanticOption } from "../../types/models";
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";
import WorkflowToolbar from "./WorkflowToolbar.vue";
import WorkflowNode from "./WorkflowNode.vue";

const workflows = useWorkflowsStore();
const { fitView, onPaneReady } = useVueFlow();
const contextMenu = ref<null | { kind: "node"; id: string; x: number; y: number; deletable: boolean } | { kind: "edge"; id: string; x: number; y: number }>(null);
const lastPointer = ref({ x: 0, y: 0 });
const pendingConnect = ref<null | { connection: any; x: number; y: number; options: WorkflowEdgeSemanticOption[] }>(null);
const edgeEditor = ref<null | (WorkflowEdgeEditorDraft & { x: number; y: number })>(null);
const workflowNodeIds = computed(() => {
  const nodes = workflows.workflowDraft.definition?.nodes;
  return Array.isArray(nodes) ? nodes.map((node: any) => String(node.id ?? "")).filter(Boolean) : [];
});
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
  contextMenu.value = {
    kind: "node",
    id: node.id,
    x: mouse.clientX,
    y: mouse.clientY,
    deletable: node.data?.protected !== true
  };
}

function openEdgeMenu(event: any) {
  const mouse = event?.event as MouseEvent | undefined;
  const edge = event?.edge;
  if (!mouse || !edge?.id) return;
  mouse.preventDefault();
  contextMenu.value = { kind: "edge", id: edge.id, x: mouse.clientX, y: mouse.clientY };
}

function openEdgeEditorFromEvent(event: any) {
  const mouse = event?.event as MouseEvent | undefined;
  const edge = event?.edge;
  if (!edge?.id) return;
  mouse?.preventDefault();
  openEdgeEditorAt(edge.id, mouse?.clientX ?? lastPointer.value.x, mouse?.clientY ?? lastPointer.value.y);
}

function closeContextMenu() {
  contextMenu.value = null;
}

function closeOverlays() {
  contextMenu.value = null;
  pendingConnect.value = null;
  edgeEditor.value = null;
}

function trackPointer(event: PointerEvent) {
  lastPointer.value = { x: event.clientX, y: event.clientY };
}

function openConnectMenu(connection: any) {
  const source = connection?.source;
  const options = source ? workflows.workflowEdgeOptions(source) : [];
  if (!source || !connection?.target || options.length === 0) return;
  closeContextMenu();
  pendingConnect.value = {
    connection,
    options,
    x: lastPointer.value.x || window.innerWidth / 2,
    y: lastPointer.value.y || window.innerHeight / 2
  };
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
  openEdgeEditorAt(menu.id, menu.x, menu.y);
}

function openEdgeEditorAt(edgeId: string, x: number, y: number) {
  const draft = workflows.openEdgeEditorDraft(edgeId);
  if (!draft) return;
  edgeEditor.value = {
    ...draft,
    x,
    y
  };
  closeContextMenu();
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
