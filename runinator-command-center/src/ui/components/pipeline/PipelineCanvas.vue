<template>
  <div class="pipeline-canvas">
    <VueFlow
      class="pipeline-flow"
      :nodes="pipeline.nodes"
      :edges="pipeline.edges"
      delete-key-code="Delete"
      :select-nodes-on-drag="false"
      :min-zoom="0.2"
      :max-zoom="2"
      @connect="onConnect"
      @edge-click="onEdgeClick"
      @node-double-click="onNodeDoubleClick"
      @pane-click="pipeline.selectEdge(null)"
      @edges-change="onEdgesChange"
    >
      <template #node-pipeline="nodeProps">
        <PipelineNode v-bind="nodeProps" />
      </template>
    </VueFlow>
  </div>
</template>

<script setup lang="ts">
import { nextTick } from "vue";
import {
  VueFlow,
  useVueFlow,
  type Connection,
  type EdgeChange,
  type EdgeMouseEvent,
  type NodeMouseEvent,
} from "@vue-flow/core";
import { usePipelineStore } from "../../adapters/pinia/pipeline";
import PipelineNode from "./PipelineNode.vue";

const emit = defineEmits<(event: "open-workflow", workflowId: string) => void>();

const pipeline = usePipelineStore();
const { fitView, onPaneReady } = useVueFlow();

function onConnect(connection: Connection) {
  if (!connection.source || !connection.target) {
    return;
  }

  void pipeline.createLink(connection.source, connection.target);
}

function onEdgeClick(event: EdgeMouseEvent) {
  pipeline.selectEdge(event.edge.id);
}

function onNodeDoubleClick(event: NodeMouseEvent) {
  emit("open-workflow", event.node.id);
}

// Vue Flow signals a delete-key removal; translate it into a trigger delete.
function onEdgesChange(changes: EdgeChange[]) {
  for (const change of changes) {
    if (change.type === "remove") {
      pipeline.selectEdge(change.id);
      void pipeline.deleteSelected();
    }
  }
}

async function recenter() {
  await nextTick();
  void fitView();
}

onPaneReady(() => {
  void recenter();
});
</script>

<style scoped>
.pipeline-canvas,
.pipeline-flow {
  width: 100%;
  height: 100%;
}
</style>
