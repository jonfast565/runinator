<template>
  <div ref="rootEl" class="flex min-h-0 flex-col gap-2">
    <!-- failure reason pinned at the top for an at-a-glance "what broke and where". -->
    <div
      v-if="failure"
      class="overflow-hidden rounded-lg border border-danger-bg bg-danger-bg p-2.5 shadow-panel"
    >
      <div class="flex items-center gap-2 text-[13px] font-semibold text-danger-fg">
        <span
          class="inline-flex size-[22px] shrink-0 items-center justify-center rounded-full bg-surface text-danger-fg"
        >
          <Icon name="alert" :size="14" />
        </span>
        <span class="inline-flex items-center gap-1.5 text-danger-fg">
          Failed at
          <code
            class="rounded border border-danger-fg bg-surface px-1.5 py-px font-mono text-xs font-semibold text-danger-fg"
            >{{ failure.nodeId }}</code
          >
        </span>
        <StatusBadge :status="failure.status" />
        <span class="flex-1"></span>
        <button
          v-if="failure.message"
          type="button"
          class="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-[5px] border border-danger-fg bg-surface px-2 py-[3px] text-[11px] font-semibold text-danger-fg transition-[background,border-color] duration-100 hover:border-danger hover:bg-surface-hover"
          :title="copied ? 'Copied' : 'Copy error'"
          @click="copyFailure"
        >
          <Icon v-if="copied" name="check" :size="13" />
          <span>{{ copied ? "Copied" : "Copy" }}</span>
        </button>
      </div>
      <pre
        v-if="failure.message"
        class="mt-2 max-h-[180px] overflow-auto rounded-md border border-danger-fg bg-surface p-2 font-mono text-[11px] leading-[1.55] break-words whitespace-pre-wrap text-danger-fg"
        >{{ failure.message }}</pre
      >
    </div>

    <!-- quick status filter for long runs (opt-in via the filterable prop). -->
    <div v-if="filterable && detail && orderedNodes.length" class="flex flex-wrap gap-1.5">
      <button
        v-for="option in filterOptions"
        :key="option.id"
        type="button"
        class="cursor-pointer rounded-pill border border-border-strong bg-surface-subtle px-2 py-0.5 text-[11px] font-semibold text-fg-subtle"
        :class="
          filter === option.id
            ? 'border-accent bg-accent-soft text-accent-text [&_.rt-filter-count]:text-accent-text'
            : ''
        "
        @click="filter = option.id"
      >
        {{ option.label }}
        <span class="rt-filter-count font-tabular text-fg-faint">{{ option.count }}</span>
      </button>
    </div>

    <div v-if="!detail" class="px-0 py-2.5 text-[13px] text-fg-muted">No run selected.</div>
    <ol v-else-if="visibleNodes.length" class="m-0 min-h-0 list-none overflow-auto p-0">
      <li
        v-for="node in visibleNodes"
        :key="node.id"
        :data-node-run-id="node.id"
        class="run-timeline-item grid grid-cols-[22px_minmax(0,1fr)] gap-2"
        :class="{ 'rounded-md': isActive(node) }"
      >
        <div class="run-timeline-rail relative flex justify-center">
          <span
            class="run-timeline-dot"
            :class="timelineDotClass(node.status)"
          ></span>
        </div>
        <div class="min-w-0 pb-2">
          <button
            type="button"
            class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-transparent bg-transparent px-1.5 py-1 text-left text-fg hover:bg-surface-hover"
            :class="{
              'border-border-strong bg-accent-soft': node.node_id === selectedNodeId,
              'border-accent': isActive(node),
            }"
            @click="onSelect(node)"
          >
            <StatusBadge :status="node.status" />
            <span class="overflow-hidden font-semibold text-ellipsis whitespace-nowrap">{{
              node.node_id
            }}</span>
            <span
              v-if="executionOrdinal(node) > 1"
              class="inline-grid size-[18px] min-w-[18px] items-center justify-center rounded-full border border-border-strong bg-surface text-[11px] leading-none font-bold font-tabular text-fg-subtle"
              :title="`Execution ${executionOrdinal(node)}`"
              >{{ executionOrdinal(node) }}</span
            >
            <span
              v-if="node.attempt > 1"
              class="rounded-pill bg-warning-bg px-[7px] text-[11px] font-semibold text-warning-fg"
              title="Attempts"
              >↻ {{ node.attempt }}</span
            >
            <span
              v-if="isActive(node)"
              class="rounded-pill bg-accent-soft px-[7px] text-[11px] font-semibold text-accent-text"
              >active</span
            >
            <span class="flex-1"></span>
            <span
              v-if="nodeTiming(node)"
              class="text-[11px] font-tabular text-fg-muted"
              :class="{ 'font-semibold text-accent-text': isRunningNode(node) }"
              >{{ nodeTiming(node) }}</span
            >
            <span
              class="text-[11px] text-fg-faint transition-transform duration-[120ms]"
              :class="{ 'rotate-90': expandedId === node.id }"
              >▸</span
            >
          </button>
          <div
            v-if="previewOf(node)"
            class="mx-1.5 mt-0.5 overflow-hidden font-mono text-[11px] leading-[1.4] text-ellipsis whitespace-nowrap text-fg-subtle"
          >
            {{ previewOf(node) }}
          </div>
          <!-- quick actions on a node (feature 7) -->
          <div v-if="node.node_id === selectedNodeId" class="mx-1.5 mt-1.5 flex flex-wrap gap-1.5">
            <slot name="node-actions" :node="node" />
          </div>
          <div
            v-if="expandedId === node.id"
            class="mx-1.5 mt-1.5 rounded-md border border-border-subtle bg-surface-subtle p-2"
          >
            <!-- failed-node errors are surfaced once in the failure banner above; only show informational messages here. -->
            <template v-if="node.message && !isFailedNode(node)">
              <div class="mb-0.5 text-[10px] tracking-wide text-fg-muted uppercase">Message</div>
              <div class="text-xs break-words whitespace-pre-wrap text-fg-subtle">
                {{ formatErrorMessage(node.message) }}
              </div>
            </template>
            <template v-if="outputText(node)">
              <div
                class="mb-0.5 text-[10px] tracking-wide text-fg-muted uppercase"
                :class="{ 'mt-2': node.message && !isFailedNode(node) }"
              >
                Output
              </div>
              <pre
                class="m-0 max-h-[200px] overflow-auto rounded border border-border-subtle bg-surface-sunken p-2 font-mono text-[11px] leading-[1.45] break-words whitespace-pre-wrap"
                >{{ outputText(node) }}</pre
              >
            </template>
            <div
              class="mb-0.5 text-[10px] tracking-wide text-fg-muted uppercase"
              :class="{ 'mt-2': (node.message && !isFailedNode(node)) || outputText(node) }"
            >
              Logs
            </div>
            <pre
              class="m-0 max-h-[200px] overflow-auto rounded border border-border-subtle bg-surface-sunken p-2 font-mono text-[11px] leading-[1.45] break-words whitespace-pre-wrap"
              >{{ logState(node) }}</pre
            >
          </div>
        </div>
      </li>
    </ol>
    <div v-else class="px-0 py-2.5 text-[13px] text-fg-muted">
      {{ orderedNodes.length ? "No steps match this filter." : "No steps recorded yet." }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import StatusBadge from "./StatusBadge.vue";
import { workflowRunExtrasService } from "../../../core/services";
import { formatErrorMessage } from "../../../core/utils/format";
import type { WorkflowNodeRun, WorkflowRunDetail } from "../../../core/domain/models";

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
  return (
    Boolean(status) && !["succeeded", "failed", "canceled", "timed_out"].includes(status ?? "")
  );
});

const orderedNodes = computed(() => {
  const nodes = props.detail?.nodes ?? [];
  return [...nodes].sort((left, right) => {
    const leftCreated = Date.parse(left.created_at ?? "");
    const rightCreated = Date.parse(right.created_at ?? "");

    if (
      Number.isFinite(leftCreated) &&
      Number.isFinite(rightCreated) &&
      leftCreated !== rightCreated
    ) {
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

    if (executions <= 0) {
      continue;
    }

    const next = (totals.get(node.node_id) ?? 0) + executions;
    totals.set(node.node_id, next);
    ordinals.set(node.id, next);
  }

  return ordinals;
});

function nodeExecutionCount(node: WorkflowNodeRun): number {
  if (Number.isFinite(node.attempt) && node.attempt > 0) {
    return Math.floor(node.attempt);
  }

  return node.status === "queued" ? 0 : 1;
}

function executionOrdinal(node: WorkflowNodeRun): number {
  return executionOrdinals.value.get(node.id) ?? 0;
}

function matchesFilter(node: WorkflowNodeRun, active: TimelineFilter): boolean {
  if (active === "all") {
    return true;
  }

  if (active === "failed") {
    return FAILED_STATUSES.has(node.status);
  }

  if (active === "succeeded") {
    return node.status === "succeeded";
  }

  return RUNNING_STATUSES.has(node.status) || isActive(node);
}

const visibleNodes = computed(() =>
  orderedNodes.value.filter((node) => matchesFilter(node, filter.value)),
);

const filterOptions = computed(() => {
  const count = (id: TimelineFilter) =>
    orderedNodes.value.filter((node) => matchesFilter(node, id)).length;
  return [
    { id: "all" as const, label: "All", count: orderedNodes.value.length },
    { id: "running" as const, label: "Active", count: count("running") },
    { id: "failed" as const, label: "Failed", count: count("failed") },
    { id: "succeeded" as const, label: "OK", count: count("succeeded") },
  ];
});

const failure = computed(() => {
  const detail = props.detail;

  if (!detail) {
    return null;
  }

  const runFailed = FAILED_STATUSES.has(detail.run.status);
  const failedNode = [...orderedNodes.value]
    .reverse()
    .find((node) => FAILED_STATUSES.has(node.status));

  if (!runFailed && !failedNode) {
    return null;
  }

  if (failedNode) {
    return {
      nodeId: failedNode.node_id,
      status: failedNode.status,
      message: formatErrorMessage(failedNode.message ?? detail.run.message) || "Run failed.",
    };
  }

  return {
    nodeId: detail.run.active_node_id ?? "run",
    status: detail.run.status,
    message: formatErrorMessage(detail.run.message) || "Run failed.",
  };
});

const copied = ref(false);

async function copyFailure() {
  if (!failure.value?.message) {
    return;
  }

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

function timelineDotClass(status: string): string {
  const base =
    "relative z-[1] mt-[7px] size-[11px] rounded-full shadow-[0_0_0_2px_var(--surface)]";

  if (status === "succeeded") {
    return `${base} bg-success-fg`;
  }

  if (status === "failed" || status === "timed_out") {
    return `${base} bg-danger`;
  }

  if (status === "running" || status === "retrying") {
    return `${base} run-timeline-dot status-running bg-accent`;
  }

  if (status === "waiting" || status === "queued" || status === "debug_paused") {
    return `${base} bg-warn`;
  }

  return `${base} bg-border-strong`;
}

function previewOf(node: WorkflowNodeRun): string {
  const output = node.output_json;

  if (output === undefined || output === null) {
    return "";
  }

  const text = typeof output === "string" ? output : JSON.stringify(output);
  const oneLine = text.replace(/\s+/g, " ").trim();

  if (!oneLine || oneLine === "{}" || oneLine === '""') {
    return "";
  }

  return oneLine.length > 140 ? `${oneLine.slice(0, 140)}…` : oneLine;
}

function outputText(node: WorkflowNodeRun): string {
  const output = node.output_json;

  if (output === undefined || output === null) {
    return "";
  }

  if (typeof output === "object" && Object.keys(output).length === 0) {
    return "";
  }

  return JSON.stringify(output, null, 2);
}

function formatMs(ms: number): string {
  if (ms < 1000) {
    return `${String(ms)}ms`;
  }

  const seconds = ms / 1000;

  if (seconds < 60) {
    return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remSec = Math.round(seconds % 60);
  return remSec === 0 ? `${String(minutes)}m` : `${String(minutes)}m ${String(remSec)}s`;
}

// a node is counting up live when it has started, is still active, and has not finished.
function isRunningNode(node: WorkflowNodeRun): boolean {
  return Boolean(node.started_at) && !node.finished_at && isActive(node);
}

// finished nodes show their wall-clock duration; running nodes count up against the ticking clock.
function nodeTiming(node: WorkflowNodeRun): string {
  if (!node.started_at) {
    return "";
  }

  const start = Date.parse(node.started_at);

  if (!Number.isFinite(start)) {
    return "";
  }

  if (node.finished_at) {
    const end = Date.parse(node.finished_at);
    return Number.isFinite(end) ? formatMs(Math.max(0, end - start)) : "";
  }

  if (isActive(node)) {
    return formatMs(Math.max(0, now.value - start));
  }

  return "";
}

function logState(node: WorkflowNodeRun): string {
  if (logLoading.value.has(node.id)) {
    return "Loading logs…";
  }

  if (!(node.id in logCache.value)) {
    return "No logs for this step.";
  }

  const cached = logCache.value[node.id];
  return cached || "No logs for this step.";
}

async function loadLogs(nodeRunId: string) {
  if (nodeRunId in logCache.value || logLoading.value.has(nodeRunId)) {
    return;
  }

  logLoading.value.add(nodeRunId);

  try {
    const chunks = await workflowRunExtrasService.fetchNodeRunChunks(nodeRunId);
    logCache.value = {
      ...logCache.value,
      [nodeRunId]: chunks.map((chunk) => chunk.content).join("\n"),
    };
  } catch {
    logCache.value = { ...logCache.value, [nodeRunId]: "" };
  } finally {
    logLoading.value.delete(nodeRunId);
  }
}

function onSelect(node: WorkflowNodeRun) {
  emit("select", node.node_id);
  expandedId.value = expandedId.value === node.id ? null : node.id;

  if (expandedId.value === node.id) {
    void loadLogs(node.id);
  }
}

// auto-expand the failing step so the debug loop opens on "what broke".
watch(
  () => failure.value?.nodeId,
  (nodeId) => {
    if (!props.autoExpandFailed || !nodeId) {
      return;
    }

    const node = orderedNodes.value.find((item) => item.node_id === nodeId);

    if (node) {
      expandedId.value = node.id;
      void loadLogs(node.id);
    }
  },
  { immediate: true },
);

// a fresh run clears stale logs so a re-run does not show the previous attempt's output.
watch(
  () => props.detail?.run.id,
  () => {
    logCache.value = {};
    logLoading.value = new Set();
    expandedId.value = null;
  },
);

// keep the running step in view while a run is in flight, without yanking finished runs around.
const activeNodeRunId = computed(
  () => visibleNodes.value.find((node) => isActive(node))?.id ?? null,
);
watch(activeNodeRunId, async (id) => {
  if (!runInFlight.value || id == null) {
    return;
  }

  await nextTick();
  rootEl.value
    ?.querySelector(`[data-node-run-id="${id}"]`)
    ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
});

onMounted(() => {
  clockTimer = window.setInterval(() => {
    // only re-render while in flight; a terminal run keeps its frozen final durations.
    if (runInFlight.value) {
      now.value = Date.now();
    }
  }, 1000);
});

onBeforeUnmount(() => {
  window.clearInterval(clockTimer);
});
</script>
