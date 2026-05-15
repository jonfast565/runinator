<template>
  <div class="panel workflow-canvas-panel workflow-run-graph-panel">
    <div class="workflow-run-graph-header">
      <div>
        <h2>{{ workflows.workflowRunWorkflow?.name ?? "Workflow" }}</h2>
        <span v-if="workflows.workflowRunDetail">Run #{{ workflows.workflowRunDetail.run.id }}</span>
      </div>
      <StatusBadge :status="workflows.workflowRunDetail?.run.status" />
    </div>
    <VueFlow
      class="workflow-graph"
      :nodes="workflows.runGraphNodes"
      :edges="workflows.runGraphEdges"
      :nodes-draggable="false"
      :nodes-connectable="false"
      :elements-selectable="true"
      :edges-updatable="false"
      :zoom-on-double-click="false"
      delete-key-code=""
      @node-click="onNodeClick"
    >
      <template #node-workflow="nodeProps">
        <WorkflowNode v-bind="nodeProps" />
      </template>
    </VueFlow>
  </div>
</template>

<script setup lang="ts">
import { nextTick, watch } from "vue";
import { VueFlow, useVueFlow } from "@vue-flow/core";
import { useWorkflowsStore } from "../../stores/workflows";
import StatusBadge from "../shared/StatusBadge.vue";
import WorkflowNode from "./WorkflowNode.vue";

const workflows = useWorkflowsStore();
const { fitView, onPaneReady } = useVueFlow();

async function recenter() {
  await nextTick();
  fitView();
}

function onNodeClick(event: any) {
  const nodeId = event?.node?.id;
  if (nodeId) workflows.selectWorkflowRunNode(nodeId);
}

onPaneReady(recenter);
watch(() => workflows.selectedWorkflowRunId, recenter);
watch(() => workflows.runGraphNodes.length, recenter);
</script>
