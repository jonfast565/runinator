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

<style scoped>
.run-gantt {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.rg-empty {
  color: var(--text-muted);
  font-size: 13px;
  padding: 8px 0;
}
.rg-summary {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11px;
  color: var(--text-subtle);
}
.rg-total {
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.rg-bottleneck {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--text-muted);
}
.rg-bottleneck code {
  font-family: var(--font-mono);
  color: var(--text-subtle);
}
.rg-swatch {
  width: 10px;
  height: 10px;
  border-radius: 2px;
  background: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-ring);
}
/* the label column width is shared by the axis (via padding) and every row's grid. */
.rg-axis {
  position: relative;
  height: 14px;
  margin-left: var(--rg-label-w, 128px);
  border-bottom: 1px solid var(--border-subtle);
}
.rg-tick {
  position: absolute;
  top: 0;
  bottom: 0;
  transform: translateX(-50%);
}
.rg-tick:first-child {
  transform: none;
}
.rg-tick:last-child {
  transform: translateX(-100%);
}
.rg-tick-label {
  font-size: 10px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.rg-rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-height: 320px;
  overflow: auto;
}
.rg-row {
  display: grid;
  grid-template-columns: var(--rg-label-w, 128px) minmax(0, 1fr);
  align-items: center;
  gap: 8px;
  cursor: pointer;
  border-radius: 5px;
  padding: 1px 0;
}
.rg-row:hover {
  background: var(--surface-hover);
}
.rg-row.selected {
  background: var(--accent-soft);
}
.rg-label {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  padding-left: 4px;
}
.rg-node {
  font-size: 12px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rg-attempt {
  flex: 0 0 auto;
  color: var(--warning-fg);
  background: var(--warning-bg);
  border-radius: 999px;
  padding: 0 5px;
  font-size: 10px;
  font-weight: 600;
}
.rg-track {
  position: relative;
  height: 18px;
  border-radius: 4px;
  background: var(--surface-subtle);
  overflow: hidden;
}
.rg-grid {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background: var(--border-faint);
}
.rg-wait {
  position: absolute;
  top: 50%;
  height: 3px;
  transform: translateY(-50%);
  border-radius: 2px;
  background: repeating-linear-gradient(
    90deg,
    var(--border-strong),
    var(--border-strong) 3px,
    transparent 3px,
    transparent 6px
  );
  opacity: 0.7;
}
.rg-bar {
  position: absolute;
  top: 2px;
  bottom: 2px;
  min-width: 2px;
  border-radius: 3px;
  background: var(--border-strong);
  display: flex;
  align-items: center;
  overflow: hidden;
}
.rg-bar.status-succeeded {
  background: var(--success-fg);
}
.rg-bar.status-failed {
  background: var(--danger-solid);
}
.rg-bar.status-running {
  background: var(--accent);
}
.rg-bar.status-waiting {
  background: var(--warn-solid, var(--warning-fg));
}
.rg-bar.running {
  animation: rg-pulse 1.4s ease-in-out infinite;
}
.rg-bar.critical {
  box-shadow:
    inset 0 0 0 1px var(--surface),
    0 0 0 1px var(--accent);
}
.rg-bar-label {
  padding: 0 5px;
  font-size: 10px;
  font-weight: 600;
  color: var(--surface);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  mix-blend-mode: difference;
  filter: invert(1) grayscale(1) contrast(9);
}
@keyframes rg-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.62;
  }
}
@media (max-width: 620px) {
  .rg-axis {
    margin-left: 84px;
  }
  .rg-row {
    grid-template-columns: 84px minmax(0, 1fr);
  }
}
</style>
