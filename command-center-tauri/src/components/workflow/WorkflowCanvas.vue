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
      @node-drag-stop="workflows.onGraphNodeDragStop"
      @nodes-change="workflows.onGraphNodesChange"
      @connect="workflows.onGraphConnect"
      @edges-change="workflows.onGraphEdgesChange"
      delete-key-code="Delete"
      :snap-to-grid="true"
      :snap-grid="[15, 15]"
    >
      <template #node-workflow="nodeProps">
        <WorkflowNode v-bind="nodeProps" />
      </template>
    </VueFlow>
    <JsonEditor
      v-show="workflows.workflowEditorMode === 'json'"
      v-model="workflows.workflowJson"
      class="workflow-json-editor"
      @blur="workflows.syncWorkflowJson"
    />
  </div>
</template>

<script setup lang="ts">
import { onMounted, watch, nextTick } from "vue";
import { VueFlow, useVueFlow } from "@vue-flow/core";
import { useWorkflowsStore } from "../../stores/workflows";
import { useWorkflowRunStream } from "../../composables/useWorkflowRunStream";
import JsonEditor from "../shared/JsonEditor.vue";
import WorkflowToolbar from "./WorkflowToolbar.vue";
import WorkflowNode from "./WorkflowNode.vue";

const workflows = useWorkflowsStore();
const { fitView, onPaneReady } = useVueFlow();
useWorkflowRunStream();

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

watch(() => workflows.selectedWorkflowRunId, () => {
  recenter();
});
</script>
