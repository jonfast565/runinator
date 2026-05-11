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
      fit-view-on-init
      @node-click="workflows.onGraphNodeClick"
      @node-drag-stop="workflows.onGraphNodeDragStop"
    />
    <JsonEditor
      v-show="workflows.workflowEditorMode === 'json'"
      v-model="workflows.workflowJson"
      class="workflow-json-editor"
      @blur="workflows.syncWorkflowJson"
    />
  </div>
</template>

<script setup lang="ts">
import { VueFlow } from "@vue-flow/core";
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";
import WorkflowToolbar from "./WorkflowToolbar.vue";

const workflows = useWorkflowsStore();
</script>
