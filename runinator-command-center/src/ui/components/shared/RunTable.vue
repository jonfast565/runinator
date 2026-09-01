<template>
  <DataTable bare :compact="compact" table-class="run-table table-resize-disabled">
    <thead>
      <tr>
        <th v-if="selectable" class="run-table-select" scope="col">
          <SelectCheckbox
            :checked="allSelected"
            :indeterminate="someSelected"
            :label="allSelected ? 'Deselect all runs' : 'Select all runs'"
            @toggle="$emit('toggle-all')"
          />
        </th>
        <th>{{ listMode && showWorkflow ? (entityLabel ?? "Workflow") : "Run" }}</th>
        <th v-if="showWorkflow && !listMode">{{ entityLabel ?? "Workflow" }}</th>
        <th class="run-table-status">Status</th>
        <template v-if="!listMode">
          <th v-if="!compact" class="col-low">Trigger</th>
          <th class="col-low">Created</th>
          <th class="col-low">Started</th>
          <th>Finished</th>
        </template>
        <th v-if="deletable" class="run-table-actions"><span class="sr-only">Actions</span></th>
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
        <td v-if="selectable" class="run-table-select">
          <SelectCheckbox
            :checked="selectedSet.has(run.id)"
            :label="`Select run ${runLabel(run)}`"
            @toggle="$emit('toggle-row', run, $event)"
          />
        </td>
        <td :title="runTooltip(run)">
          <div v-if="listMode" class="run-table-summary">
            <span class="run-table-title">{{ runPrimaryLabel(run) }}</span>
            <span class="run-table-meta">{{ runMeta(run) }}</span>
          </div>
          <template v-else>{{ runLabel(run) }}</template>
        </td>
        <td v-if="showWorkflow && !listMode" :title="workflowTooltip(run)">
          {{ workflowLabel(run) }}
        </td>
        <td class="run-table-status"><StatusBadge :status="run.status" /></td>
        <template v-if="!listMode">
          <td v-if="!compact" class="col-low" :title="run.trigger_source_kind ?? undefined">
            {{ run.trigger_source_kind ?? "—" }}
          </td>
          <td class="col-low" :title="formatDate(run.created_at)">
            {{ formatDate(run.created_at) }}
          </td>
          <td class="col-low" :title="formatDate(run.started_at)">
            {{ formatDate(run.started_at) }}
          </td>
          <td :title="formatDate(run.finished_at)">{{ formatDate(run.finished_at) }}</td>
        </template>
        <td v-if="deletable" class="run-table-actions" @click.stop>
          <button
            class="btn btn-icon btn-ghost"
            title="Delete run"
            aria-label="Delete run"
            @click="$emit('delete', run)"
          >
            <Icon name="trash" />
          </button>
        </td>
      </tr>
    </tbody>
  </DataTable>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { RunSummary } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { isBadStatus, isGoodStatus } from "../../../core/utils/status";
import Icon from "./Icon.vue";
import SelectCheckbox from "./SelectCheckbox.vue";
import StatusBadge from "./StatusBadge.vue";

const props = withDefaults(
  defineProps<{
    runs: RunSummary[];
    selectedRunId: string | null;
    compact?: boolean;
    // master-list layout: folds workflow, trigger, and timing context into the primary run cell.
    listMode?: boolean;
    showWorkflow?: boolean;
    workflowNames?: Record<string, string>;
    // header label for the entity column (default "Workflow"); set to "Pipeline" for pipeline runs.
    entityLabel?: string;
    // leading checkbox column; selection state is owned by the caller (useBulkSelection).
    selectable?: boolean;
    selectedRunIds?: string[];
    allSelected?: boolean;
    someSelected?: boolean;
    deletable?: boolean;
  }>(),
  {
    compact: false,
    listMode: false,
    showWorkflow: false,
    workflowNames: undefined,
    entityLabel: undefined,
    selectable: false,
    selectedRunIds: () => [],
    allSelected: false,
    someSelected: false,
    deletable: false,
  },
);

defineEmits<{
  select: [run: RunSummary];
  "toggle-row": [run: RunSummary, event: MouseEvent];
  "toggle-all": [];
  delete: [run: RunSummary];
}>();

const selectedSet = computed(() => new Set(props.selectedRunIds));

function workflowLabel(run: RunSummary): string {
  if (!run.workflow_id) {
    return "Unknown workflow";
  }

  const name = props.workflowNames?.[run.workflow_id];
  return name ?? `Workflow #${shortId(run.workflow_id)}`;
}

function runLabel(run: RunSummary): string {
  const name = run.name?.trim();
  return name ? `${name} · #${shortId(run.id)}` : `Run #${shortId(run.id)}`;
}

function runMeta(run: RunSummary): string {
  return [
    secondaryRunLabel(run),
    run.trigger_source_kind ?? "",
    formatDate(run.started_at ?? run.created_at),
  ]
    .filter(Boolean)
    .join(" · ");
}

function runPrimaryLabel(run: RunSummary): string {
  return props.showWorkflow ? workflowLabel(run) : runLabel(run);
}

function secondaryRunLabel(run: RunSummary): string {
  const name = run.name?.trim();
  return name ? `${name} · Run #${shortId(run.id)}` : `Run #${shortId(run.id)}`;
}

function runTooltip(run: RunSummary): string {
  const name = run.name?.trim();
  let heading = name ?? `Run ${run.id}`;

  if (!heading) {
    heading = `Run ${run.id}`;
  }

  return [
    heading,
    `Run ID: ${run.id}`,
    props.showWorkflow && run.workflow_id ? `Workflow: ${workflowTooltip(run)}` : "",
    run.trigger_source_kind ? `Trigger: ${run.trigger_source_kind}` : "",
    `Created: ${formatDate(run.created_at)}`,
  ]
    .filter(Boolean)
    .join("\n");
}

function workflowTooltip(run: RunSummary): string {
  if (!run.workflow_id) {
    return "Unknown workflow";
  }

  const name = props.workflowNames?.[run.workflow_id];
  return name ? `${name} (${run.workflow_id})` : run.workflow_id;
}

function shortId(value: string): string {
  return value.length > 8 ? value.slice(0, 8) : value;
}
</script>

<style scoped>
:deep(.run-table) {
  width: 100%;
  table-layout: fixed;
}

:deep(.run-table-select) {
  width: 36px;
  padding-right: 4px;
  padding-left: 8px;
}

:deep(.run-table-status) {
  width: 96px;
}

:deep(.run-table-actions) {
  width: 44px;
  padding-right: 6px;
  padding-left: 4px;
  text-align: right;
}

.run-table-summary {
  display: grid;
  min-width: 0;
  gap: 2px;
  padding: 2px 0;
}

.run-table-title,
.run-table-meta {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.run-table-title {
  color: var(--text);
  font-weight: 600;
}

.run-table-meta {
  color: var(--text-muted);
  font-size: 11px;
}
</style>
