<template>
  <div class="run-gantt">
    <div v-if="!layout.rows.length" class="rg-empty">No timing recorded yet.</div>
    <template v-else>
      <div class="rg-summary">
        <span class="rg-total">Total {{ formatDuration(layout.totalMs) }}</span>
        <span v-if="layout.bottleneckNodeId" class="rg-bottleneck" title="Longest step (critical path)">
          <span class="rg-swatch"></span>
          Bottleneck: <code>{{ layout.bottleneckNodeId }}</code>
        </span>
      </div>

      <!-- shared time axis -->
      <div class="rg-axis">
        <div
          v-for="tick in layout.ticks"
          :key="tick.pct"
          class="rg-tick"
          :style="{ left: `${tick.pct}%` }"
        >
          <span class="rg-tick-label">{{ tick.label }}</span>
        </div>
      </div>

      <ol class="rg-rows">
        <li
          v-for="row in layout.rows"
          :key="row.id"
          class="rg-row"
          :class="{ selected: row.nodeId === selectedNodeId }"
          @click="emit('select', row.nodeId)"
        >
          <div class="rg-label" :title="row.nodeId">
            <span class="rg-node">{{ row.nodeId }}</span>
            <span v-if="row.attempt > 1" class="rg-attempt" title="Attempts">↻{{ row.attempt }}</span>
          </div>
          <div class="rg-track">
            <!-- gridlines aligned to the axis ticks -->
            <span
              v-for="tick in layout.ticks"
              :key="`g-${tick.pct}`"
              class="rg-grid"
              :style="{ left: `${tick.pct}%` }"
            ></span>
            <!-- queued/parked time before the node went active -->
            <span
              v-if="row.waitWidthPct > 0.4"
              class="rg-wait"
              :style="{ left: `${row.waitLeftPct}%`, width: `${row.waitWidthPct}%` }"
              :title="`Waited ${formatDuration(row.waitMs)}`"
            ></span>
            <!-- active segment -->
            <span
              class="rg-bar"
              :class="[statusBadgeClass(row.status), { critical: row.critical, running: row.running }]"
              :style="{ left: `${row.barLeftPct}%`, width: `${row.barWidthPct}%` }"
              :title="barTitle(row)"
            >
              <span v-if="row.durationMs > 0" class="rg-bar-label">{{
                formatDuration(row.durationMs)
              }}</span>
            </span>
          </div>
        </li>
      </ol>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { statusBadgeClass } from "../../../core/utils/status";
import {
  buildGanttLayout,
  formatDuration,
  type GanttRow,
} from "../../../core/workflow/run-gantt";
import type { WorkflowRunDetail } from "../../../core/domain/models";

const props = defineProps<{
  detail: WorkflowRunDetail | null;
  selectedNodeId?: string | null;
}>();

const emit = defineEmits<{ select: [nodeId: string] }>();

// ticks once a second while the run is in flight so active bars count up.
const now = ref(Date.now());
let clockTimer = 0;

const runInFlight = computed(() => {
  const status = props.detail?.run.status;
  return (
    Boolean(status) && !["succeeded", "failed", "canceled", "timed_out"].includes(status ?? "")
  );
});

const layout = computed(() => buildGanttLayout(props.detail, now.value));

function barTitle(row: GanttRow): string {
  const active = `${row.nodeId} · ${formatDuration(row.durationMs)}`;
  return row.waitMs > 0 ? `${active} (waited ${formatDuration(row.waitMs)})` : active;
}

onMounted(() => {
  clockTimer = window.setInterval(() => {
    if (runInFlight.value) {
      now.value = Date.now();
    }
  }, 1000);
});

onBeforeUnmount(() => {
  window.clearInterval(clockTimer);
});
</script>

