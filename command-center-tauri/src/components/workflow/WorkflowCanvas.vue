<template>
  <div class="panel workflow-canvas-panel">
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
      @connect="workflows.onGraphConnect"
      @edge-update="workflows.onGraphEdgeUpdate"
      @edge-context-menu="openEdgeMenu"
      @edges-change="workflows.onGraphEdgesChange"
      @pane-click="closeContextMenu"
      @pane-context-menu="closeContextMenu"
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
      <button v-if="contextMenu.kind === 'edge'" @click="deleteContextEdge">Delete edge</button>
    </div>
    <JsonEditor
      v-show="workflows.workflowEditorMode === 'json'"
      v-model="workflows.workflowJson"
      class="workflow-json-editor"
      @blur="workflows.syncWorkflowJson"
    />
  </div>
</template>

<script setup lang="ts">
import { watch, nextTick, ref } from "vue";
import { VueFlow, useVueFlow } from "@vue-flow/core";
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";
import WorkflowToolbar from "./WorkflowToolbar.vue";
import WorkflowNode from "./WorkflowNode.vue";

const workflows = useWorkflowsStore();
const { fitView, onPaneReady } = useVueFlow();
const contextMenu = ref<null | { kind: "node"; id: string; x: number; y: number; deletable: boolean } | { kind: "edge"; id: string; x: number; y: number }>(null);

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

function closeContextMenu() {
  contextMenu.value = null;
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
</script>
