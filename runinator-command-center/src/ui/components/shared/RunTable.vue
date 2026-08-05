<template>
  <table :class="{ compact }">
    <thead>
      <tr>
        <th v-if="selectable" class="w-9" scope="col">
          <SelectCheckbox
            :checked="allSelected"
            :indeterminate="someSelected"
            :label="allSelected ? 'Deselect all runs' : 'Select all runs'"
            @toggle="$emit('toggle-all')"
          />
        </th>
        <th>Run</th>
        <th v-if="showWorkflow">{{ entityLabel ?? "Workflow" }}</th>
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
        <td v-if="selectable" class="w-9">
          <SelectCheckbox
            :checked="selectedSet.has(run.id)"
            :label="`Select run ${runLabel(run)}`"
            @toggle="$emit('toggle-row', run, $event)"
          />
        </td>
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
import { computed } from "vue";
import type { RunSummary } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { isBadStatus, isGoodStatus } from "../../../core/utils/status";
import SelectCheckbox from "./SelectCheckbox.vue";
import StatusBadge from "./StatusBadge.vue";

const props = withDefaults(
  defineProps<{
    runs: RunSummary[];
    selectedRunId: string | null;
    compact?: boolean;
    showWorkflow?: boolean;
    workflowNames?: Record<string, string>;
    // header label for the entity column (default "Workflow"); set to "Pipeline" for pipeline runs.
    entityLabel?: string;
    // leading checkbox column; selection state is owned by the caller (useBulkSelection).
    selectable?: boolean;
    selectedRunIds?: string[];
    allSelected?: boolean;
    someSelected?: boolean;
  }>(),
  {
    compact: false,
    showWorkflow: false,
    workflowNames: undefined,
    entityLabel: undefined,
    selectable: false,
    selectedRunIds: () => [],
    allSelected: false,
    someSelected: false,
  },
);

defineEmits<{
  select: [run: RunSummary];
  "toggle-row": [run: RunSummary, event: MouseEvent];
  "toggle-all": [];
}>();

const selectedSet = computed(() => new Set(props.selectedRunIds));

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
