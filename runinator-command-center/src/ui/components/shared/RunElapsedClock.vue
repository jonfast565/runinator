<template>
  <div class="elapsed-clock" role="timer" :aria-label="accessibleDuration" aria-live="off">
    <svg class="clock-face" viewBox="0 0 120 120" aria-hidden="true">
      <circle class="clock-rim" cx="60" cy="60" r="57" />
      <line
        v-for="tick in 60"
        :key="tick"
        x1="60"
        :y1="tick % 5 === 0 ? 8 : 10"
        x2="60"
        :y2="tick % 5 === 0 ? 14 : 12"
        :transform="`rotate(${tick * 6} 60 60)`"
        class="clock-tick"
        :class="{ major: tick % 5 === 0 }"
      />
      <text x="60" y="27">12</text>
      <text x="96" y="64">3</text>
      <text x="60" y="102">6</text>
      <text x="24" y="64">9</text>
      <line
        class="clock-hand hour"
        x1="60"
        y1="63"
        x2="60"
        y2="36"
        :style="handStyle(seconds / 120)"
      />
      <line
        class="clock-hand minute"
        x1="60"
        y1="65"
        x2="60"
        y2="23"
        :style="handStyle(seconds / 10)"
      />
      <line
        class="clock-hand second"
        x1="60"
        y1="69"
        x2="60"
        y2="18"
        :style="handStyle(seconds * 6)"
      />
      <circle class="clock-pin" cx="60" cy="60" r="3" />
    </svg>
    <div class="clock-caption">
      <span class="clock-label">{{ terminal ? "Final duration" : "Elapsed time" }}</span>
      <strong class="clock-duration">{{ duration }}</strong>
      <span class="clock-note">{{ note }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { isTerminalWorkflowRunStatus } from "../../../core/utils/status";

const props = defineProps<{
  startedAt: string | null;
  finishedAt: string | null;
  status: string;
}>();
const now = ref(Date.now());
const mounted = ref(false);
onMounted(() => {
  mounted.value = true;
});
const start = computed(() => Date.parse(props.startedAt ?? ""));
const finish = computed(() => Date.parse(props.finishedAt ?? ""));
const terminal = computed(
  () => isTerminalWorkflowRunStatus(props.status) || Number.isFinite(finish.value),
);
const ticking = computed(() => Number.isFinite(start.value) && !terminal.value);
const unavailable = computed(
  () => terminal.value && (!Number.isFinite(start.value) || !Number.isFinite(finish.value)),
);
const seconds = computed(() => {
  if (!Number.isFinite(start.value) || unavailable.value) {
    return 0;
  }

  return Math.max(
    0,
    Math.floor(((terminal.value ? finish.value : now.value) - start.value) / 1000),
  );
});
const hours = computed(() => Math.floor(seconds.value / 3600));
const minutes = computed(() => Math.floor(seconds.value / 60) % 60);
const duration = computed(() =>
  unavailable.value
    ? "—"
    : [hours.value, minutes.value, seconds.value % 60]
        .map((value) => String(value).padStart(2, "0"))
        .join(":"),
);
const note = computed(() =>
  unavailable.value
    ? "Timing unavailable"
    : !Number.isFinite(start.value)
      ? "Waiting to start"
      : terminal.value
        ? "Run finished"
        : "Since run started",
);
const accessibleDuration = computed(() =>
  unavailable.value
    ? "Final duration unavailable"
    : `${note.value}: ${String(hours.value)} hours, ${String(minutes.value)} minutes, ${String(seconds.value % 60)} seconds`,
);
const handStyle = (degrees: number) => ({ transform: `rotate(${String(degrees)}deg)` });

watch(
  [mounted, ticking, () => props.startedAt],
  ([ready, active], _previous, onCleanup) => {
    now.value = Date.now();

    if (!ready || !active) {
      return;
    }

    const timer = window.setInterval(() => {
      now.value = Date.now();
    }, 100);
    onCleanup(() => {
      window.clearInterval(timer);
    });
  },
  { immediate: true },
);
</script>

<style scoped>
.elapsed-clock {
  display: flex;
  align-items: center;
  gap: 1rem;
  min-width: 0;
}
.clock-face {
  width: 104px;
  height: 104px;
  flex: none;
}
.clock-rim {
  fill: var(--surface);
  stroke: var(--border);
  stroke-width: 1.5;
}
.clock-tick {
  stroke: var(--text-muted);
  stroke-width: 1;
}
.clock-tick.major {
  stroke: var(--text);
  stroke-width: 1.5;
}
text {
  fill: var(--text-muted);
  font-size: 11px;
  text-anchor: middle;
  font-family: inherit;
}
.clock-hand {
  transform-origin: 60px 60px;
  stroke-linecap: round;
}
.hour {
  stroke: var(--text);
  stroke-width: 4;
}
.minute {
  stroke: var(--text);
  stroke-width: 2.5;
}
.second {
  stroke: var(--accent);
  stroke-width: 1.5;
}
.clock-pin {
  fill: var(--accent);
}
.clock-caption {
  display: grid;
  gap: 0.2rem;
  min-width: 0;
}
.clock-label,
.clock-note {
  color: var(--text-muted);
  font-size: 0.75rem;
}
.clock-duration {
  color: var(--text);
  font-size: 1.4rem;
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}
</style>
