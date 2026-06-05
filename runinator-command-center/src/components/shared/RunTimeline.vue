<template>
  <div class="run-timeline">
    <!-- failure reason pinned at the top for an at-a-glance "what broke and where". -->
    <div v-if="failure" class="rt-failure">
      <div class="rt-failure-head">
        <Icon name="alert" :size="14" />
        <span>Failed at <strong>{{ failure.nodeId }}</strong></span>
        <StatusBadge :status="failure.status" />
      </div>
      <div v-if="failure.message" class="rt-failure-msg">{{ failure.message }}</div>
    </div>

    <!-- quick status filter for long runs (opt-in via the filterable prop). -->
    <div v-if="filterable && detail && orderedNodes.length" class="rt-filters">
      <button
        v-for="option in filterOptions"
        :key="option.id"
        type="button"
        class="rt-filter"
        :class="{ active: filter === option.id }"
        @click="filter = option.id"
      >
        {{ option.label }} <span class="rt-filter-count">{{ option.count }}</span>
      </button>
    </div>

    <div v-if="!detail" class="rt-empty">No run selected.</div>
    <ol v-else-if="visibleNodes.length" class="rt-list">
      <li
        v-for="node in visibleNodes"
        :key="node.id"
        :class="['rt-item', { selected: node.node_id === selectedNodeId, active: isActive(node) }]"
      >
        <div class="rt-rail">
          <span class="rt-dot" :class="statusBadgeClass(node.status)"></span>
        </div>
        <div class="rt-body">
          <button type="button" class="rt-head" @click="onSelect(node)">
            <StatusBadge :status="node.status" />
            <span class="rt-node-id">{{ node.node_id }}</span>
            <span v-if="node.attempt > 1" class="rt-attempt" title="Attempts">↻ {{ node.attempt }}</span>
            <span v-if="isActive(node)" class="rt-active">active</span>
            <span class="rt-spacer"></span>
            <span v-if="nodeTiming(node)" class="rt-duration" :class="{ live: isRunningNode(node) }">{{ nodeTiming(node) }}</span>
            <span class="rt-caret" :class="{ open: expandedId === node.id }">▸</span>
          </button>
          <div v-if="previewOf(node)" class="rt-preview">{{ previewOf(node) }}</div>
          <!-- quick actions on a node (feature 7) -->
          <div v-if="node.node_id === selectedNodeId" class="rt-actions">
            <slot name="node-actions" :node="node" />
          </div>
          <div v-if="expandedId === node.id" class="rt-expand">
            <template v-if="node.message">
              <div class="rt-expand-label">Message</div>
              <div class="rt-message">{{ node.message }}</div>
            </template>
            <template v-if="outputText(node)">
              <div class="rt-expand-label">Output</div>
              <pre class="rt-json">{{ outputText(node) }}</pre>
            </template>
            <div class="rt-expand-label">Logs</div>
            <pre class="rt-logs">{{ logState(node) }}</pre>
          </div>
        </div>
      </li>
    </ol>
    <div v-else class="rt-empty">{{ orderedNodes.length ? "No steps match this filter." : "No steps recorded yet." }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import StatusBadge from "./StatusBadge.vue";
import { fetchWorkflowNodeRunChunks } from "../../api/commandCenterApi";
import { statusBadgeClass } from "../../utils/status";
import type { WorkflowNodeRun, WorkflowRunDetail } from "../../types/models";

const props = defineProps<{
  detail: WorkflowRunDetail | null;
  selectedNodeId?: string | null;
  // when true, the most recent failed node is expanded automatically.
  autoExpandFailed?: boolean;
  // when true, show a status filter bar above the steps.
  filterable?: boolean;
}>();

type TimelineFilter = "all" | "running" | "failed" | "succeeded";
const filter = ref<TimelineFilter>("all");

const emit = defineEmits<{
  select: [nodeId: string];
}>();

const RUNNING_STATUSES = new Set(["running", "waiting", "queued", "retrying"]);
const FAILED_STATUSES = new Set(["failed", "timed_out"]);

const expandedId = ref<number | null>(null);
const logCache = ref<Record<number, string>>({});
const logLoading = ref<Set<number>>(new Set());
// ticks once a second while the run is in flight so active-node elapsed times count up.
const now = ref(Date.now());
let clockTimer = 0;

const runInFlight = computed(() => {
  const status = props.detail?.run.status;
  return Boolean(status) && !["succeeded", "failed", "canceled", "timed_out"].includes(status ?? "");
});

// steps in execution order; node-run id is monotonic so it doubles as a stable ordering.
const orderedNodes = computed(() => {
  const nodes = props.detail?.nodes ?? [];
  return [...nodes].sort((left, right) => left.id - right.id);
});

function matchesFilter(node: WorkflowNodeRun, active: TimelineFilter): boolean {
  if (active === "all") return true;
  if (active === "failed") return FAILED_STATUSES.has(node.status);
  if (active === "succeeded") return node.status === "succeeded";
  return RUNNING_STATUSES.has(node.status) || isActive(node);
}

const visibleNodes = computed(() => orderedNodes.value.filter((node) => matchesFilter(node, filter.value)));

const filterOptions = computed(() => {
  const count = (id: TimelineFilter) => orderedNodes.value.filter((node) => matchesFilter(node, id)).length;
  return [
    { id: "all" as const, label: "All", count: orderedNodes.value.length },
    { id: "running" as const, label: "Active", count: count("running") },
    { id: "failed" as const, label: "Failed", count: count("failed") },
    { id: "succeeded" as const, label: "OK", count: count("succeeded") }
  ];
});

const failure = computed(() => {
  const detail = props.detail;
  if (!detail) return null;
  const runFailed = FAILED_STATUSES.has(detail.run.status);
  const failedNode = [...orderedNodes.value].reverse().find((node) => FAILED_STATUSES.has(node.status));
  if (!runFailed && !failedNode) return null;
  if (failedNode) {
    return {
      nodeId: failedNode.node_id,
      status: failedNode.status,
      message: failedNode.message || detail.run.message || "Run failed."
    };
  }
  return { nodeId: detail.run.active_node_id ?? "run", status: detail.run.status, message: detail.run.message ?? "Run failed." };
});

function isActive(node: WorkflowNodeRun): boolean {
  if (props.detail?.run.active_node_id && props.detail.run.active_node_id === node.node_id) {
    return !FAILED_STATUSES.has(node.status) && node.status !== "succeeded";
  }
  return RUNNING_STATUSES.has(node.status);
}

function previewOf(node: WorkflowNodeRun): string {
  const output = node.output_json;
  if (output === undefined || output === null) return "";
  const text = typeof output === "string" ? output : JSON.stringify(output);
  const oneLine = text.replace(/\s+/g, " ").trim();
  if (!oneLine || oneLine === "{}" || oneLine === '""') return "";
  return oneLine.length > 140 ? `${oneLine.slice(0, 140)}…` : oneLine;
}

function outputText(node: WorkflowNodeRun): string {
  const output = node.output_json;
  if (output === undefined || output === null) return "";
  if (typeof output === "object" && Object.keys(output).length === 0) return "";
  return JSON.stringify(output, null, 2);
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  const minutes = Math.floor(seconds / 60);
  const remSec = Math.round(seconds % 60);
  return remSec === 0 ? `${minutes}m` : `${minutes}m ${remSec}s`;
}

// a node is counting up live when it has started, is still active, and has not finished.
function isRunningNode(node: WorkflowNodeRun): boolean {
  return Boolean(node.started_at) && !node.finished_at && isActive(node);
}

// finished nodes show their wall-clock duration; running nodes count up against the ticking clock.
function nodeTiming(node: WorkflowNodeRun): string {
  if (!node.started_at) return "";
  const start = Date.parse(node.started_at);
  if (!Number.isFinite(start)) return "";
  if (node.finished_at) {
    const end = Date.parse(node.finished_at);
    return Number.isFinite(end) ? formatMs(Math.max(0, end - start)) : "";
  }
  if (isActive(node)) return formatMs(Math.max(0, now.value - start));
  return "";
}

function logState(node: WorkflowNodeRun): string {
  if (logLoading.value.has(node.id)) return "Loading logs…";
  const cached = logCache.value[node.id];
  if (cached === undefined) return "No logs for this step.";
  return cached || "No logs for this step.";
}

async function loadLogs(nodeRunId: number) {
  if (logCache.value[nodeRunId] !== undefined || logLoading.value.has(nodeRunId)) return;
  logLoading.value.add(nodeRunId);
  try {
    const chunks = await fetchWorkflowNodeRunChunks(nodeRunId);
    logCache.value = { ...logCache.value, [nodeRunId]: chunks.map((chunk) => chunk.content).join("") };
  } catch {
    logCache.value = { ...logCache.value, [nodeRunId]: "" };
  } finally {
    logLoading.value.delete(nodeRunId);
  }
}

function onSelect(node: WorkflowNodeRun) {
  emit("select", node.node_id);
  expandedId.value = expandedId.value === node.id ? null : node.id;
  if (expandedId.value === node.id) void loadLogs(node.id);
}

// auto-expand the failing step so the debug loop opens on "what broke".
watch(
  () => failure.value?.nodeId,
  (nodeId) => {
    if (!props.autoExpandFailed || !nodeId) return;
    const node = orderedNodes.value.find((item) => item.node_id === nodeId);
    if (node) {
      expandedId.value = node.id;
      void loadLogs(node.id);
    }
  },
  { immediate: true }
);

// a fresh run clears stale logs so a re-run does not show the previous attempt's output.
watch(
  () => props.detail?.run.id,
  () => {
    logCache.value = {};
    logLoading.value = new Set();
    expandedId.value = null;
  }
);

onMounted(() => {
  clockTimer = window.setInterval(() => {
    // only re-render while in flight; a terminal run keeps its frozen final durations.
    if (runInFlight.value) now.value = Date.now();
  }, 1000);
});

onBeforeUnmount(() => window.clearInterval(clockTimer));
</script>

<style scoped>
.run-timeline {
  display: flex;
  flex-direction: column;
  min-height: 0;
  gap: 8px;
}
.rt-failure {
  border: 1px solid #f3c2c2;
  border-left: 3px solid #dc2626;
  border-radius: 6px;
  background: #fff5f5;
  padding: 8px 10px;
}
.rt-failure-head {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #b91c1c;
  font-weight: 600;
  font-size: 13px;
}
.rt-failure-msg {
  margin-top: 4px;
  color: #9f1239;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
}
.rt-empty {
  color: #66717e;
  font-size: 13px;
  padding: 10px 0;
}
.rt-filters {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.rt-filter {
  border: 1px solid #c8d1db;
  border-radius: 999px;
  background: #f8fafc;
  color: #4b5663;
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 9px;
}
.rt-filter.active {
  border-color: #2563eb;
  background: #eef5ff;
  color: #1d4ed8;
}
.rt-filter-count {
  color: #97a1ad;
  font-variant-numeric: tabular-nums;
}
.rt-filter.active .rt-filter-count {
  color: #2563eb;
}
.rt-list {
  list-style: none;
  margin: 0;
  padding: 0;
  overflow: auto;
  min-height: 0;
}
.rt-item {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr);
  gap: 8px;
}
.rt-rail {
  display: flex;
  justify-content: center;
  position: relative;
}
.rt-rail::before {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background: #e3e8ee;
}
.rt-item:first-child .rt-rail::before {
  top: 12px;
}
.rt-item:last-child .rt-rail::before {
  bottom: calc(100% - 16px);
}
.rt-dot {
  position: relative;
  z-index: 1;
  width: 11px;
  height: 11px;
  margin-top: 7px;
  border-radius: 50%;
  background: #cbd5e1;
  box-shadow: 0 0 0 2px #fff;
}
.rt-dot.status-succeeded {
  background: #16a34a;
}
.rt-dot.status-failed {
  background: #dc2626;
}
.rt-dot.status-running {
  background: #2563eb;
  animation: rt-pulse 1.2s ease-in-out infinite;
}
.rt-dot.status-waiting {
  background: #d97706;
}
.rt-caret {
  color: #94a3b8;
  font-size: 11px;
  transition: transform 0.12s ease;
}
.rt-caret.open {
  transform: rotate(90deg);
}
@keyframes rt-pulse {
  0%, 100% { box-shadow: 0 0 0 2px #fff, 0 0 0 4px rgba(37, 99, 235, 0.25); }
  50% { box-shadow: 0 0 0 2px #fff, 0 0 0 7px rgba(37, 99, 235, 0); }
}
.rt-body {
  min-width: 0;
  padding-bottom: 8px;
}
.rt-item.active .rt-body {
  border-radius: 6px;
}
.rt-head {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  padding: 4px 6px;
  cursor: pointer;
  text-align: left;
  color: #17202b;
}
.rt-head:hover {
  background: #f1f5fb;
}
.rt-item.selected .rt-head {
  border-color: #b7c8dc;
  background: #eef5ff;
}
.rt-item.active .rt-head {
  border-color: #bcd0ef;
}
.rt-node-id {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rt-attempt {
  color: #8a5a00;
  background: #fff4cc;
  border-radius: 999px;
  padding: 0 7px;
  font-size: 11px;
  font-weight: 600;
}
.rt-active {
  color: #1d4ed8;
  background: #e0ecff;
  border-radius: 999px;
  padding: 0 7px;
  font-size: 11px;
  font-weight: 600;
}
.rt-spacer {
  flex: 1 1 auto;
}
.rt-duration {
  color: #66717e;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.rt-duration.live {
  color: #1d4ed8;
  font-weight: 600;
}
.rt-preview {
  margin: 2px 6px 0;
  color: #4d5d70;
  font: 11px/1.4 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rt-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 6px 6px 0;
}
.rt-expand {
  margin: 6px 6px 0;
  border: 1px solid #e3e8ee;
  border-radius: 6px;
  padding: 8px;
  background: #fbfcfe;
}
.rt-expand-label {
  color: #66717e;
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin: 0 0 3px;
}
.rt-expand-label + .rt-expand-label,
.rt-json + .rt-expand-label,
.rt-message + .rt-expand-label {
  margin-top: 8px;
}
.rt-message {
  color: #9f1239;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
}
.rt-json,
.rt-logs {
  margin: 0;
  max-height: 200px;
  overflow: auto;
  background: #fff;
  border: 1px solid #e6ebf1;
  border-radius: 4px;
  padding: 6px 8px;
  font: 11px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
