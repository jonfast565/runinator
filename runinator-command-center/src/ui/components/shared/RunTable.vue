<template>
  <table :class="{ compact }">
    <thead>
      <tr>
        <th>Run</th>
        <th v-if="showWorkflow">Workflow</th>
        <th>Status</th>
        <th v-if="!compact" class="col-low">Trigger</th>
        <th class="col-low">Created</th>
        <th class="col-low">Started</th>
        <th>Finished</th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="run in runs"
        :key="run.id"
        :class="{
          selected: run.id === selectedRunId,
          danger: isBadStatus(run.status),
          success: isGoodStatus(run.status),
        }"
        @click="$emit('select', run)"
      >
        <td>{{ runLabel(run) }}</td>
        <td v-if="showWorkflow">{{ workflowLabel(run) }}</td>
        <td><StatusBadge :status="run.status" /></td>
        <td v-if="!compact" class="col-low">{{ run.trigger ?? "" }}</td>
        <td class="col-low">{{ formatDate(run.created_at) }}</td>
        <td class="col-low">{{ formatDate(run.started_at) }}</td>
        <td>{{ formatDate(run.finished_at) }}</td>
      </tr>
    </tbody>
  </table>
</template>

<script setup lang="ts">
import type { RunSummary } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { isBadStatus, isGoodStatus } from "../../../core/utils/status";
import StatusBadge from "./StatusBadge.vue";

const props = defineProps<{
  runs: RunSummary[];
  selectedRunId: string | null;
  compact?: boolean;
  showWorkflow?: boolean;
  workflowNames?: Record<string, string>;
}>();

defineEmits<{
  select: [run: RunSummary];
}>();

function workflowLabel(run: RunSummary): string {
  if (!run.workflow_id) {
    return "-";
  }

  const name = props.workflowNames?.[run.workflow_id];
  return name ? `${name} #${run.workflow_id}` : run.workflow_id;
}

function runLabel(run: RunSummary): string {
  const name = run.name?.trim();
  return name ? `${name} (#${run.id})` : run.id;
}
</script>
