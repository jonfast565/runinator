<template>
  <div class="log-panel">
    <div class="log-controls">
      <input
        v-model="filter"
        class="log-filter-input"
        placeholder="Filter logs (substring)"
      />
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
      Severity is inferred client-side from stream (stderr → error) and substring match — best-effort.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { RunChunk } from "../../types/models";

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
  if (timer !== undefined) window.clearInterval(timer);
});

const isLive = computed(() => {
  if (!props.lastChunkAt) return false;
  return now.value - props.lastChunkAt < 3000;
});

type Line = { text: string; stream: string };

const filteredLines = computed<Line[]>(() => {
  const query = filter.value.toLowerCase().trim();
  const lines: Line[] = [];
  if (props.chunks.length === 0 && props.fallbackText) {
    for (const text of props.fallbackText.split("\n")) {
      lines.push({ text, stream: "stdout" });
    }
  } else {
    for (const chunk of props.chunks) {
      const stream = chunk.stream ?? "stdout";
      if (stream === "stdout" && !showStdout.value) continue;
      if (stream === "stderr" && !showStderr.value) continue;
      lines.push({ text: `[${stream}] ${chunk.content}`, stream });
    }
  }
  if (!query) return lines;
  return lines.filter((line) => line.text.toLowerCase().includes(query));
});

function lineClass(line: Line): string {
  if (line.stream === "stderr") return "log-error";
  const upper = line.text.toUpperCase();
  if (upper.includes("ERROR") || upper.includes("FATAL")) return "log-error";
  if (upper.includes("WARN")) return "log-warn";
  if (upper.includes("DEBUG")) return "log-debug";
  return "";
}
</script>

<style scoped>
.log-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}
.log-controls {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 6px;
  font-size: 11px;
}
.log-filter-input {
  flex: 1;
  padding: 3px 8px;
  border: 1px solid #ccd4dd;
  border-radius: 4px;
  font-size: 11px;
}
.log-toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: #475569;
  cursor: pointer;
}
.log-tail-indicator {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: #94a3b8;
  font-size: 10px;
  text-transform: uppercase;
  font-weight: 600;
}
.log-tail-indicator .dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #cbd5e1;
}
.log-tail-indicator.live {
  color: #16a34a;
}
.log-tail-indicator.live .dot {
  background: #22c55e;
  animation: pulse 1.2s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
.log-output {
  flex: 1;
  font-size: 11px;
  font-family: "SFMono-Regular", Consolas, monospace;
  background: #0f172a;
  color: #e2e8f0;
  padding: 8px;
  margin: 0;
  border-radius: 4px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}
.log-output .log-error {
  color: #fca5a5;
}
.log-output .log-warn {
  color: #fcd34d;
}
.log-output .log-debug {
  color: #94a3b8;
}
.log-hint {
  font-size: 10px;
  color: #94a3b8;
  margin: 4px 0 0;
}
</style>
