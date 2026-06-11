<template>
  <div ref="rootEl" class="run-timeline">
    <!-- failure reason pinned at the top for an at-a-glance "what broke and where". -->
    <div v-if="failure" class="rt-failure">
      <div class="rt-failure-head">
        <span class="rt-failure-icon"><Icon name="alert" :size="14" /></span>
        <span class="rt-failure-title">Failed at <code class="rt-failure-node">{{ failure.nodeId }}</code></span>
        <StatusBadge :status="failure.status" />
        <span class="rt-spacer"></span>
        <button v-if="failure.message" type="button" class="rt-failure-copy" :title="copied ? 'Copied' : 'Copy error'" @click="copyFailure">
          <Icon v-if="copied" name="check" :size="13" />
          <span>{{ copied ? "Copied" : "Copy" }}</span>
        </button>
      </div>
      <pre v-if="failure.message" class="rt-failure-msg">{{ failure.message }}</pre>
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
        :data-node-run-id="node.id"
        :class="['rt-item', { selected: node.node_id === selectedNodeId, active: isActive(node) }]"
      >
        <div class="rt-rail">
          <span class="rt-dot" :class="statusBadgeClass(node.status)"></span>
        </div>
        <div class="rt-body">
          <button type="button" class="rt-head" @click="onSelect(node)">
            <StatusBadge :status="node.status" />
            <span class="rt-node-id">{{ node.node_id }}</span>
            <span v-if="executionOrdinal(node) > 1" class="rt-execution" :title="`Execution ${executionOrdinal(node)}`">{{ executionOrdinal(node) }}</span>
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
            <!-- failed-node errors are surfaced once in the failure banner above; only show informational messages here. -->
            <template v-if="node.message && !isFailedNode(node)">
              <div class="rt-expand-label">Message</div>
              <div class="rt-message">{{ formatErrorMessage(node.message) }}</div>
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
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import StatusBadge from "./StatusBadge.vue";
import { fetchWorkflowNodeRunChunks } from "../../api/commandCenterApi";
import { statusBadgeClass } from "../../utils/status";
import { formatErrorMessage } from "../../utils/format";
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

const rootEl = ref<HTMLElement | null>(null);
const expandedId = ref<string | null>(null);
const logCache = ref<Record<string, string>>({});
const logLoading = ref<Set<string>>(new Set());
// ticks once a second while the run is in flight so active-node elapsed times count up.
const now = ref(Date.now());
let clockTimer = 0;

const runInFlight = computed(() => {
  const status = props.detail?.run.status;
  return Boolean(status) && !["succeeded", "failed", "canceled", "timed_out"].includes(status ?? "");
});

const orderedNodes = computed(() => {
  const nodes = props.detail?.nodes ?? [];
  return [...nodes].sort((left, right) => {
    const leftCreated = Date.parse(left.created_at ?? "");
    const rightCreated = Date.parse(right.created_at ?? "");
    if (Number.isFinite(leftCreated) && Number.isFinite(rightCreated) && leftCreated !== rightCreated) {
      return leftCreated - rightCreated;
    }
    // created_at has second precision, so same-second steps tie; the uuidv7 run id is
    // time-ordered at finer precision and string-sorts chronologically.
    return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  });
});

const executionOrdinals = computed(() => {
  const totals = new Map<string, number>();
  const ordinals = new Map<string, number>();
  for (const node of orderedNodes.value) {
    const executions = nodeExecutionCount(node);
    if (executions <= 0) continue;
    const next = (totals.get(node.node_id) ?? 0) + executions;
    totals.set(node.node_id, next);
    ordinals.set(node.id, next);
  }
  return ordinals;
});

function nodeExecutionCount(node: WorkflowNodeRun): number {
  if (Number.isFinite(node.attempt) && node.attempt > 0) return Math.floor(node.attempt);
  return node.status === "queued" ? 0 : 1;
}

function executionOrdinal(node: WorkflowNodeRun): number {
  return executionOrdinals.value.get(node.id) ?? 0;
}

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
      message: formatErrorMessage(failedNode.message || detail.run.message) || "Run failed."
    };
  }
  return {
    nodeId: detail.run.active_node_id ?? "run",
    status: detail.run.status,
    message: formatErrorMessage(detail.run.message) || "Run failed."
  };
});

const copied = ref(false);
async function copyFailure() {
  if (!failure.value?.message) return;
  try {
    await navigator.clipboard.writeText(failure.value.message);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1200);
  } catch {
    // clipboard may be unavailable; ignore.
  }
}

function isActive(node: WorkflowNodeRun): boolean {
  if (props.detail?.run.active_node_id && props.detail.run.active_node_id === node.node_id) {
    return !FAILED_STATUSES.has(node.status) && node.status !== "succeeded";
  }
  return RUNNING_STATUSES.has(node.status);
}

function isFailedNode(node: WorkflowNodeRun): boolean {
  return FAILED_STATUSES.has(node.status);
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

async function loadLogs(nodeRunId: string) {
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

// keep the running step in view while a run is in flight, without yanking finished runs around.
const activeNodeRunId = computed(() => visibleNodes.value.find((node) => isActive(node))?.id ?? null);
watch(activeNodeRunId, async (id) => {
  if (!runInFlight.value || id == null) return;
  await nextTick();
  rootEl.value?.querySelector(`[data-node-run-id="${id}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
});

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
  border: 1px solid #f4cccc;
  border-radius: 8px;
  background: linear-gradient(180deg, #fff6f6 0%, #fffafa 100%);
  box-shadow: 0 1px 2px rgba(190, 18, 60, 0.06);
  padding: 10px 12px;
  overflow: hidden;
}
.rt-failure-head {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #b42318;
  font-weight: 600;
  font-size: 13px;
}
.rt-failure-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: 0 0 auto;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: #fee4e2;
  color: #d92d20;
}
.rt-failure-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: #7a271a;
}
.rt-failure-node {
  border: 1px solid #f3c6c0;
  border-radius: 4px;
  background: #fff;
  color: #b42318;
  padding: 1px 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  font-weight: 600;
}
.rt-failure-copy {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
  border: 1px solid #f0bcbc;
  border-radius: 5px;
  background: #fff;
  color: #b42318;
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  padding: 3px 9px;
  transition: background 0.12s ease, border-color 0.12s ease;
}
.rt-failure-copy:hover {
  background: #fef2f1;
  border-color: #e69a93;
}
.rt-failure-msg {
  margin: 8px 0 0;
  max-height: 180px;
  overflow: auto;
  border: 1px solid #f6dada;
  border-radius: 6px;
  background: #fffafa;
  padding: 8px 10px;
  color: #912018;
  font: 11px/1.55 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
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
.rt-execution {
  display: inline-grid;
  min-width: 18px;
  height: 18px;
  align-items: center;
  justify-content: center;
  border: 1px solid #b7c8dc;
  border-radius: 50%;
  background: #ffffff;
  color: #334155;
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1;
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
  color: #44505f;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
}
.rt-message.error {
  border-left: 3px solid #dc2626;
  border-radius: 4px;
  background: #fff5f5;
  color: #9f1239;
  padding: 6px 8px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
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
