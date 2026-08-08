<template>
  <div class="panel workflow-canvas-panel workflow-run-graph-panel">
    <div class="workflow-run-graph-header">
      <div>
        <h2>{{ workflows.workflowRunWorkflow?.name ?? "Workflow" }}</h2>
        <span v-if="workflows.workflowRunDetail"
          >Run #{{ workflows.workflowRunDetail.run.id }}</span
        >
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
      <template #edge-workflow="edgeProps">
        <WorkflowEdge v-bind="edgeProps" />
      </template>
    </VueFlow>
  </div>
</template>

<script setup lang="ts">
import { nextTick, watch } from "vue";
import { VueFlow, useVueFlow, type NodeMouseEvent } from "@vue-flow/core";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import StatusBadge from "../shared/StatusBadge.vue";
import WorkflowNode from "./WorkflowNode.vue";
import WorkflowEdge from "./WorkflowEdge.vue";

const workflows = useWorkflowsStore();
const { fitView, onPaneReady } = useVueFlow();

async function recenter() {
  await nextTick();
  void fitView();
}

function onNodeClick(event: NodeMouseEvent) {
  const nodeId = event.node.id;

  if (!nodeId) {
    return;
  }

  const native = event.event as MouseEvent | undefined;

  if (native?.shiftKey && workflows.isDebugRun) {
    void workflows.toggleBreakpoint(nodeId);
    return;
  }

  workflows.selectWorkflowRunNode(nodeId);
}

onPaneReady(() => {
  void recenter();
});
watch(
  () => workflows.selectedWorkflowRunId,
  () => {
    void recenter();
  },
);
watch(
  () => workflows.runGraphNodes.length,
  () => {
    void recenter();
  },
);
// follow only the *selected* thread of control, and only when asked.
//
// this used to watch the run's single `current_node_id` and refit on every change. with several
// branches stepping independently that fires on every step of every branch, so the camera thrashes
// between them; scoping it to one cursor behind an opt-in toggle is what makes a forked session
// readable.
watch(
  () => {
    const selected = workflows.cursorMarkers.find((cursor) => cursor.selected);
    return selected?.paused ? selected.nodeId : null;
  },
  async (nodeId) => {
    if (!workflows.followCursor || typeof nodeId !== "string" || !nodeId) {
      return;
    }

    await nextTick();
    void fitView({ nodes: [nodeId], duration: 350 });
  },
);
</script>
