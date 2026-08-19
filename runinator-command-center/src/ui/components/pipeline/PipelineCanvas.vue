<template>
  <div class="pipeline-canvas">
    <VueFlow
      class="pipeline-flow"
      :nodes="displayNodes"
      :edges="displayEdges"
      :delete-key-code="readonly ? undefined : 'Delete'"
      :select-nodes-on-drag="false"
      :min-zoom="0.2"
      :max-zoom="2"
      @connect="onConnect"
      @edge-click="onEdgeClick"
      @node-click="onNodeClick"
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
import { computed, nextTick } from "vue";
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
import type { PipelineRunDetail } from "../../../core/domain/models";
import type { PipelineEdgeModel, PipelineNodeData, PipelineNodeModel } from "../../../core/workflow/pipeline-graph";

const props = withDefaults(defineProps<{ detail?: PipelineRunDetail | null; readonly?: boolean }>(), {
  detail: null,
  readonly: false,
});
const readonly = computed(() => props.readonly || props.detail != null);
const emit = defineEmits<(event: "open-workflow" | "open-run", id: string) => void>();

const pipeline = usePipelineStore();
const { fitView, onPaneReady } = useVueFlow();

const latestAttempts = computed(() => {
  const latest = new Map<string, PipelineRunDetail["attempts"][number]>();

  for (const attempt of props.detail?.attempts ?? []) {
    const prior = latest.get(attempt.member_key);
    if (!prior || attempt.attempt > prior.attempt) {latest.set(attempt.member_key, attempt);}
  }

  return latest;
});

function duration(started: string | null, finished: string | null): string | undefined {
  if (!started) {return undefined;}
  const milliseconds = new Date(finished ?? Date.now()).getTime() - new Date(started).getTime();
  if (!Number.isFinite(milliseconds) || milliseconds < 0) {return undefined;}
  return milliseconds < 60_000 ? `${String(Math.round(milliseconds / 1000))}s` : `${String(Math.round(milliseconds / 60_000))}m`;
}

const displayNodes = computed<PipelineNodeModel[]>(() => {
  const snapshot = props.detail?.run.pipeline_snapshot;
  if (!snapshot) {return pipeline.nodes;}
  return snapshot.graph.members.map((member, index) => {
    const attempt = latestAttempts.value.get(member.key);
    const envelope = attempt?.result as { result?: unknown; artifacts?: unknown[] } | null;
    return {
      id: member.key,
      type: "pipeline",
      position: { x: (index % 3) * 260, y: Math.floor(index / 3) * 150 },
      data: {
        workflowId: member.workflow_id, name: member.key, enabled: true,
        incoming: snapshot.graph.links.filter((link) => link.enabled && link.to === member.key).length,
        outgoing: snapshot.graph.links.filter((link) => link.enabled && link.from === member.key).length,
        failureMode: member.failure_mode, status: attempt?.status ?? "pending", attempt: attempt?.attempt,
        duration: duration(attempt?.started_at ?? null, attempt?.finished_at ?? null),
        hasResult: envelope?.result != null, artifactCount: envelope?.artifacts?.length ?? 0,
        message: attempt?.message, workflowRunId: attempt?.workflow_run_id,
      },
    };
  });
});

const displayEdges = computed<PipelineEdgeModel[]>(() => {
  const detail = props.detail;
  const snapshot = detail?.run.pipeline_snapshot;
  if (!snapshot) {return pipeline.edges;}
  const states = new Map(detail.edges.map((edge) => [edge.link_id, edge.state]));
  return snapshot.graph.links.map((link) => {
    const state = states.get(link.id) ?? "pending";
    return {
      id: link.id, type: "pipeline", source: link.from,
      target: link.to, label: `${link.on} · ${state}`,
      animated: state === "active", class: `pipeline-edge-${state}`,
      data: { triggerId: link.id, sourceWorkflowId: snapshot.graph.members.find((member) => member.key === link.from)?.workflow_id ?? link.from,
        targetName: link.to, on: link.on, enabled: link.enabled, parameters: link.parameters },
    };
  });
});

function onConnect(connection: Connection) {
  if (readonly.value) {return;}

  if (!connection.source || !connection.target) {
    return;
  }

  void pipeline.createLink(connection.source, connection.target);
}

function onEdgeClick(event: EdgeMouseEvent) {
  if (readonly.value) {return;}
  pipeline.selectEdge(event.edge.id);
}

function onNodeClick(event: NodeMouseEvent) {
  if (readonly.value) {return;}
  pipeline.selectNode(event.node.id);
}

function onNodeDoubleClick(event: NodeMouseEvent) {
  const data = event.node.data as PipelineNodeData;
  const workflowRunId = data.workflowRunId;

  if (readonly.value && workflowRunId) {emit("open-run", workflowRunId);}
  else {emit("open-workflow", data.workflowId);}
}

// Vue Flow signals a delete-key removal; translate it into a trigger delete.
function onEdgesChange(changes: EdgeChange[]) {
  if (readonly.value) {return;}

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

:deep(.pipeline-edge-satisfied path) { stroke: var(--success-fg, #067647); }
:deep(.pipeline-edge-skipped path) { stroke-dasharray: 5 5; opacity: 0.5; }
:deep(.pipeline-edge-active path) { stroke: var(--accent, #6941c6); }
</style>
