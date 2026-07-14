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
    </div>
    <Handle
      class="pipeline-handle pipeline-handle-source"
      type="source"
      :position="Position.Right"
    />
  </div>
</template>

<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import type { PipelineNodeData } from "../../../core/workflow/pipeline-graph";

defineProps<{ data: PipelineNodeData }>();
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

.pipeline-handle {
  width: 9px;
  height: 9px;
  background: var(--accent, #6941c6);
  border: 2px solid var(--surface, #ffffff);
}
</style>
