<template>
  <div class="log-panel">
    <div class="log-controls">
      <input v-model="filter" class="log-filter-input" placeholder="Filter logs (substring)" />
      <label class="log-toggle">
        <input v-model="showStdout" type="checkbox" />
        stdout
      </label>
      <label class="log-toggle">
        <input v-model="showStderr" type="checkbox" />
        stderr
      </label>
      <label class="log-toggle">
        <input v-model="colorize" type="checkbox" />
        color
      </label>
      <span class="log-tail-indicator" :class="{ live: isLive }">
        <span class="dot" />
        {{ isLive ? "live" : "idle" }}
      </span>
      <HelpBubble
        text="Every line keeps the durable output timestamp, stream, retry attempt, effect, and continuation that emitted it. Severity is inferred client-side from stream and message text, so it is best-effort."
        label="About log severity"
      />
    </div>
    <div v-if="context" class="log-context" :class="contextTone">
      <div class="log-context-title">
        <strong>{{ context.nodeId || "Run diagnostics" }}</strong>
        <span>{{ context.nodeStatus || context.runStatus }}</span>
        <span v-if="context.effectStatus">effect {{ context.effectStatus }}</span>
        <span v-if="context.attempt !== null">attempt {{ context.attempt }}</span>
      </div>
      <div class="log-context-meta">
        <span>run {{ shortId(context.runId) }}</span>
        <span v-if="context.effectId">effect {{ shortId(context.effectId) }}</span>
        <span v-if="context.continuationId">thread {{ shortId(context.continuationId) }}</span>
        <span v-if="context.action">{{ context.action }}</span>
      </div>
      <pre v-if="context.message" class="log-context-message">{{ context.message }}</pre>
    </div>
    <div v-if="chunks.length === 0" class="log-empty-state">
      <strong>No streamed output was recorded for this step.</strong>
      <span>
        The execution diagnostic above is durable metadata; provider output appears here only when
        the worker emits stdout or stderr chunks.
      </span>
    </div>
    <pre class="log-output">
<span
  v-for="line in filteredLines"
  :key="line.id"
  :class="colorize ? lineClass(line) : ''"
><span class="log-line-meta">{{ line.timestamp }} · {{ line.stream }} · attempt {{ line.attempt }} · effect {{ shortId(line.effectId) }}</span> {{ line.content }}
</span>
    </pre>
  </div>
</template>

<script lang="ts">
export interface LogContext {
  runId: string;
  runStatus: string;
  nodeId?: string | null;
  nodeStatus?: string | null;
  effectId?: string | null;
  effectStatus?: string | null;
  continuationId?: string | null;
  attempt?: number | null;
  action?: string | null;
  message?: string | null;
}
</script>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { RunChunk } from "../../../core/domain/models";
import { outputChunkLines, outputChunkTimestamp } from "../../../core/workflow/output-chunks";
import HelpBubble from "../shared/HelpBubble.vue";

const props = defineProps<{
  chunks: RunChunk[];
  lastChunkAt: number;
  context?: LogContext | null;
}>();

const filter = ref("");
const showStdout = ref(true);
const showStderr = ref(true);
const colorize = ref(true);
const now = ref(Date.now());
let timer: number | undefined;

onMounted(() => {
  timer = window.setInterval(() => {
    now.value = Date.now();
  }, 1000);
});

onBeforeUnmount(() => {
  if (timer !== undefined) {
    window.clearInterval(timer);
  }
});

const isLive = computed(() => {
  if (!props.lastChunkAt) {
    return false;
  }

  return now.value - props.lastChunkAt < 3000;
});

interface Line {
  id: string;
  timestamp: string;
  stream: string;
  attempt: number;
  effectId: string;
  continuationId: string;
  content: string;
}

const filteredLines = computed<Line[]>(() => {
  const query = filter.value.toLowerCase().trim();
  const lines = outputChunkLines(props.chunks)
    .filter((line) => (line.stream === "stdout" ? showStdout.value : true))
    .filter((line) => (line.stream === "stderr" ? showStderr.value : true))
    .map((line) => ({ ...line, timestamp: outputChunkTimestamp(line.timestamp) }));

  if (!query) {
    return lines;
  }

  return lines.filter((line) =>
    [line.timestamp, line.stream, String(line.attempt), line.effectId, line.continuationId, line.content]
      .join(" ")
      .toLowerCase()
      .includes(query),
  );
});

const contextTone = computed(() => {
  const status = `${props.context?.nodeStatus ?? ""} ${props.context?.effectStatus ?? ""}`;
  return /failed|timed_out|canceled|rejected/.test(status) ? "is-error" : "";
});

function lineClass(line: Line): string {
  if (line.stream === "stderr") {
    return "log-error";
  }

  const upper = line.content.toUpperCase();

  if (upper.includes("ERROR") || upper.includes("FATAL")) {
    return "log-error";
  }

  if (upper.includes("WARN")) {
    return "log-warn";
  }

  if (upper.includes("DEBUG")) {
    return "log-debug";
  }

  return "";
}

function shortId(value: string): string {
  return value.length > 12 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value;
}

</script>
