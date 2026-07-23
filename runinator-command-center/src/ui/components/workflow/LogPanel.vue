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
    </div>
    <pre class="log-output">
<span
  v-for="(line, idx) in filteredLines"
  :key="idx"
  :class="colorize ? lineClass(line) : ''"
>{{ line.text }}
</span>
    </pre>
    <p class="log-hint">
      Severity is inferred client-side from stream (stderr → error) and substring match —
      best-effort.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { RunChunk } from "../../../core/domain/models";

const props = defineProps<{
  chunks: RunChunk[];
  lastChunkAt: number;
  fallbackText?: string;
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
  text: string;
  stream: string;
}

const filteredLines = computed<Line[]>(() => {
  const query = filter.value.toLowerCase().trim();
  const lines: Line[] = [];

  if (props.chunks.length === 0 && props.fallbackText) {
    for (const text of props.fallbackText.split("\n")) {
      lines.push({ text, stream: "stdout" });
    }
  } else {
    for (const chunk of props.chunks) {
      const stream = chunk.stream;

      if (stream === "stdout" && !showStdout.value) {
        continue;
      }

      if (stream === "stderr" && !showStderr.value) {
        continue;
      }

      lines.push({ text: `[${stream}] ${chunk.content}`, stream });
    }
  }

  if (!query) {
    return lines;
  }

  return lines.filter((line) => line.text.toLowerCase().includes(query));
});

function lineClass(line: Line): string {
  if (line.stream === "stderr") {
    return "log-error";
  }

  const upper = line.text.toUpperCase();

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
</script>

