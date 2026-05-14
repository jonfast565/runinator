<template>
  <table :class="{ compact }">
    <thead>
      <tr>
        <th>Run</th>
        <th>Status</th>
        <th v-if="!compact">Trigger</th>
        <th>Created</th>
        <th>Started</th>
        <th>Finished</th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="run in runs"
        :key="run.id"
        :class="{ selected: run.id === selectedRunId, danger: isBadStatus(run.status), success: isGoodStatus(run.status) }"
        @click="$emit('select', run)"
      >
        <td>{{ run.id }}</td>
        <td><StatusBadge :status="run.status" /></td>
        <td v-if="!compact">{{ run.trigger ?? "" }}</td>
        <td>{{ formatDate(run.created_at) }}</td>
        <td>{{ formatDate(run.started_at) }}</td>
        <td>{{ formatDate(run.finished_at) }}</td>
      </tr>
    </tbody>
  </table>
</template>

<script setup lang="ts">
import type { RunSummary } from "../../types/models";
import { formatDate } from "../../utils/format";
import { isBadStatus, isGoodStatus } from "../../utils/status";
import StatusBadge from "./StatusBadge.vue";

defineProps<{
  runs: RunSummary[];
  selectedRunId: number;
  compact?: boolean;
}>();

defineEmits<{
  select: [run: RunSummary];
}>();
</script>
