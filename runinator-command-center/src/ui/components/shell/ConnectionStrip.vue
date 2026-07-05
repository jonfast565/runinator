<template>
  <div ref="root" class="connection-cluster">
    <!-- on mobile the full summary collapses to one tappable status chip that reveals a popover. -->
    <button
      class="connection-chip"
      :class="connectionPillClass"
      aria-label="Connection status"
      :aria-expanded="popoverOpen"
      @click="popoverOpen = !popoverOpen"
    >
      <span class="connection-chip-dot"></span>
      <span class="connection-chip-label">{{ connectionPillLabel }}</span>
    </button>
    <div class="connection-summary" :class="{ 'popover-open': popoverOpen }">
      <span class="service-url" :title="app.serviceLabel">{{ app.serviceLabel }}</span>
      <span class="connection-pill" :class="connectionPillClass">
        {{ connectionPillLabel }}
      </span>
      <span class="connection-pill stream-state" :class="app.eventStreamState">
        <span v-if="app.eventStreamState === 'connected'" class="stream-state-dot"></span>
        {{ app.eventStreamLabel }}
      </span>
      <span v-if="!app.isRealtime && app.lastRefreshAt" class="last-refresh"
        >Last refresh: {{ app.lastRefreshText }}</span
      >
      <span v-if="app.hasReplicaState" class="replica-summary">
        {{ app.liveReplicaCount }}/{{ app.replicas.length }} healthy ·
        {{ app.replicaCounts.webservices }} ws · {{ app.replicaCounts.workers }} workers ·
        {{ app.replicaCounts.wakers }} wakers
      </span>
      <div v-if="supervisor.status.value?.configured" class="supervisor-pills">
        <span
          v-for="proc in supervisor.status.value?.processes ?? []"
          :key="proc.name"
          class="supervisor-pill"
          :class="pillClass(proc.status, supervisor.status.value?.stale_seconds)"
          :title="processTooltip(proc)"
        >
          {{ proc.name }}
        </span>
        <span v-if="staleHint" class="supervisor-stale">{{ staleHint }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useSupervisorStatus } from "../../composables/useSupervisorStatus";
import type { SupervisorProcessSnapshot } from "../../../core/api/commandCenterApi";

const app = useAppStore();
const supervisor = useSupervisorStatus();

const root = ref<HTMLElement | null>(null);
// popover is only visually active on mobile (css-gated); track open state here for the chip toggle.
const popoverOpen = ref(false);

function onDocumentClick(event: MouseEvent) {
  if (popoverOpen.value && root.value && !root.value.contains(event.target as Node)) {
    popoverOpen.value = false;
  }
}

onMounted(() => {
  document.addEventListener("click", onDocumentClick);
});
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
});

// three-way tag: reachable (green), known-but-unreachable (red), or not yet discovered (amber).
const connectionPillClass = computed(() => {
  if (app.serviceConnected) {
    return "connected";
  }

  if (app.serviceKnown) {
    return "down";
  }

  return "waiting";
});
const connectionPillLabel = computed(() => {
  if (app.serviceConnected) {
    return "Service up";
  }

  if (app.serviceKnown) {
    return "Service down";
  }

  return "Service pending";
});

const staleHint = computed(() => {
  const seconds = supervisor.status.value?.stale_seconds;

  if (seconds == null || seconds < 30) {
    return "";
  }

  return `state ${String(seconds)}s old`;
});

function pillClass(status: string, staleSeconds: number | null | undefined) {
  const stale = staleSeconds != null && staleSeconds > 30;

  if (stale) {
    return "supervisor-pill-stale";
  }

  const normalized = status.toLowerCase();

  if (normalized === "running") {
    return "supervisor-pill-running";
  }

  if (normalized === "starting" || normalized === "backoff") {
    return "supervisor-pill-warn";
  }

  if (normalized === "failed" || normalized === "exited" || normalized === "stopping") {
    return "supervisor-pill-fail";
  }

  return "supervisor-pill-neutral";
}

function processTooltip(proc: SupervisorProcessSnapshot): string {
  const parts: string[] = [];
  parts.push(`status: ${proc.status}`);

  if (proc.pid != null) {
    parts.push(`pid ${String(proc.pid)}`);
  }

  if (proc.uptime_seconds != null) {
    parts.push(`uptime ${formatUptime(proc.uptime_seconds)}`);
  }

  if (proc.restarts > 0) {
    parts.push(`${String(proc.restarts)} restarts`);
  }

  if (proc.last_exit_code != null) {
    parts.push(`last exit ${String(proc.last_exit_code)}`);
  }

  if (proc.last_error) {
    parts.push(proc.last_error);
  }

  return parts.join(" · ");
}

function formatUptime(seconds: number): string {
  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  const m = Math.floor(seconds / 60);

  if (m < 60) {
    return `${String(m)}m`;
  }

  const h = Math.floor(m / 60);
  return `${String(h)}h${String(m % 60)}m`;
}
</script>

<style scoped>
.connection-cluster {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
}

/* the chip is a mobile-only summary control; desktop shows the inline summary instead. */
.connection-chip {
  display: none;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: var(--radius-pill);
  font-size: 12px;
}

.connection-chip-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
}

.connection-chip.connected {
  color: var(--success-fg);
}

.connection-chip.down {
  color: var(--danger-fg);
}

.connection-chip.waiting {
  color: var(--warning-fg);
}
.connection-summary {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  min-width: 0;
  max-width: 100%;
  gap: 6px;
  overflow: hidden;
  flex-wrap: nowrap;
}
.connection-cluster .service-url {
  max-width: clamp(120px, 18vw, 220px);
  flex: 0 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.replica-summary {
  overflow: hidden;
  color: var(--text-muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.stream-state {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
}

.stream-state.connected {
  background: var(--success-bg);
  color: var(--success-fg);
}

/* a pulse means "this is live right now" (actively streaming), distinct from a settled ok state. */
.stream-state-dot {
  position: relative;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent-pulse);
  flex: 0 0 auto;
}

.stream-state-dot::after {
  content: "";
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  background: var(--accent-pulse-soft);
  animation: stream-state-pulse 1.6s ease-out infinite;
}

@keyframes stream-state-pulse {
  0% {
    transform: scale(0.6);
    opacity: 0.7;
  }

  70%,
  100% {
    transform: scale(1.8);
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .stream-state-dot::after {
    animation: none;
  }
}

.stream-state.connecting,
.stream-state.fallback {
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.stream-state.disconnected {
  background: var(--surface-muted);
  color: var(--text-muted);
}
.supervisor-pills {
  display: flex;
  align-items: center;
  min-width: 0;
  overflow: hidden;
  gap: 4px;
}
.supervisor-pill {
  flex: 0 0 auto;
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 10px;
  border: 1px solid transparent;
  font-weight: 500;
}
.supervisor-pill-running {
  background: var(--success-bg);
  color: var(--success-fg);
  border-color: var(--success-fg);
}
.supervisor-pill-warn {
  background: var(--warning-bg);
  color: var(--warning-fg);
  border-color: var(--warning-fg);
}
.supervisor-pill-fail {
  background: var(--danger-bg);
  color: var(--danger-fg);
  border-color: var(--danger-fg);
}
.supervisor-pill-neutral {
  background: var(--surface-muted);
  color: var(--text-subtle);
  border-color: var(--border);
}
.supervisor-pill-stale {
  background: var(--surface-muted);
  color: var(--text-faint);
  border-color: var(--border-subtle);
}
.supervisor-stale {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--text-faint);
  margin-left: 4px;
}

@media (max-width: 760px) {
  .connection-chip {
    display: inline-flex;
  }

  /* the inline summary becomes a popover, hidden until the chip toggles it open. */
  .connection-summary {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 45;
    display: none;
    flex-direction: column;
    align-items: flex-start;
    flex-wrap: wrap;
    width: max-content;
    max-width: 78vw;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
    overflow: visible;
  }

  .connection-summary.popover-open {
    display: flex;
  }

  .connection-cluster .service-url {
    max-width: 100%;
  }

  /* the plain status pill duplicates the chip; keep the stream-state pill in the popover. */
  .connection-summary .connection-pill:not(.stream-state) {
    display: none;
  }
}
</style>
