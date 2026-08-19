<template>
  <div class="pipeline-node" :class="{ 'pipeline-node-disabled': !data.enabled }">
    <Handle
      class="pipeline-handle pipeline-handle-target"
      type="target"
      :position="Position.Left"
    />
    <div class="pipeline-node-title">{{ data.name }}</div>
    <div class="pipeline-node-meta">
      <span v-if="!data.enabled" class="pipeline-node-badge pipeline-node-badge-muted">disabled</span>
      <span v-if="data.incoming" class="pipeline-node-badge" title="incoming chains">
        ← {{ data.incoming }}
      </span>
      <span v-if="data.outgoing" class="pipeline-node-badge" title="outgoing chains">
        {{ data.outgoing }} →
      </span>
      <span
        v-if="data.failureMode !== 'continue'"
        class="pipeline-node-badge pipeline-node-badge-failure-mode"
        :title="`On failure: ${data.failureMode}`"
      >
        {{ failureModeLabel[data.failureMode] }}
      </span>
      <span v-if="data.status" class="pipeline-node-badge pipeline-node-badge-status">
        {{ data.status }}<template v-if="data.attempt && data.attempt > 1"> · #{{ data.attempt }}</template>
      </span>
      <span v-if="data.duration" class="pipeline-node-badge">{{ data.duration }}</span>
      <span v-if="data.hasResult" class="pipeline-node-badge">result</span>
      <span v-if="data.artifactCount" class="pipeline-node-badge">{{ data.artifactCount }} artifact{{ data.artifactCount === 1 ? "" : "s" }}</span>
    </div>
    <div v-if="data.message" class="pipeline-node-message" :title="data.message">{{ data.message }}</div>
    <Handle
      class="pipeline-handle pipeline-handle-source"
      type="source"
      :position="Position.Right"
    />
  </div>
</template>

<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import type { PipelineMemberFailureMode } from "../../../core/domain/models";
import type { PipelineNodeData } from "../../../core/workflow/pipeline-graph";

defineProps<{ data: PipelineNodeData }>();

const failureModeLabel: Record<PipelineMemberFailureMode, string> = {
  stop: "stop on failure",
  continue: "continue on failure",
  silently_continue: "silently continue",
  inquire: "inquire on failure",
};
</script>

<style scoped>
.pipeline-node {
  min-width: 150px;
  max-width: 220px;
  padding: 10px 14px;
  border-radius: 10px;
  border: 1px solid var(--border, #d0d5dd);
  background: var(--surface, #ffffff);
  box-shadow: 0 1px 2px rgba(16, 24, 40, 0.08);
  font-size: 13px;
}

.pipeline-node-disabled {
  opacity: 0.6;
  border-style: dashed;
}

.pipeline-node-title {
  font-weight: 600;
  color: var(--text, #101828);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pipeline-node-meta {
  display: flex;
  gap: 6px;
  margin-top: 6px;
  flex-wrap: wrap;
}

.pipeline-node-badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--surface-muted, #f2f4f7);
  color: var(--text-muted, #475467);
}

.pipeline-node-badge-muted {
  background: transparent;
  border: 1px solid var(--border, #d0d5dd);
}

.pipeline-node-badge-failure-mode {
  background: var(--warning-bg, #fff2cc);
  color: var(--warning-fg, #84620d);
}

.pipeline-node-badge-status { font-weight: 600; }
.pipeline-node-message { margin-top: 6px; max-width: 190px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--danger-fg, #b42318); font-size: 11px; }

.pipeline-handle {
  width: 9px;
  height: 9px;
  background: var(--accent, #6941c6);
  border: 2px solid var(--surface, #ffffff);
}
</style>
