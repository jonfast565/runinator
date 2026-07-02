<template>
  <div class="connection-cluster">
    <div class="connection-summary">
      <span class="service-url" :title="app.serviceLabel">{{ app.serviceLabel }}</span>
      <span class="connection-pill" :class="{ connected: app.serviceConnected, waiting: !app.serviceConnected }">
        {{ app.serviceConnected ? "Service up" : "Service pending" }}
      </span>
      <span class="connection-pill stream-state" :class="app.eventStreamState">{{ app.eventStreamLabel }}</span>
      <span v-if="!app.isRealtime && app.lastRefreshAt" class="last-refresh">Last refresh: {{ app.lastRefreshText }}</span>
      <span v-if="app.hasReplicaState" class="replica-summary">
        {{ app.liveReplicaCount }}/{{ app.replicas.length }} healthy ·
        {{ app.replicaCounts.webservices }} ws ·
        {{ app.replicaCounts.workers }} workers ·
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
import { computed } from "vue";
import { useAppStore } from "../../stores/app";
import { useSupervisorStatus } from "../../composables/useSupervisorStatus";
import type { SupervisorProcessSnapshot } from "../../api/commandCenterApi";

const app = useAppStore();
const supervisor = useSupervisorStatus();

const staleHint = computed(() => {
  const seconds = supervisor.status.value?.stale_seconds;
  if (seconds == null || seconds < 30) return "";
  return `state ${seconds}s old`;
});

function pillClass(status: string, staleSeconds: number | null | undefined) {
  const stale = staleSeconds != null && staleSeconds > 30;
  if (stale) return "supervisor-pill-stale";
  const normalized = status.toLowerCase();
  if (normalized === "running") return "supervisor-pill-running";
  if (normalized === "starting" || normalized === "backoff") return "supervisor-pill-warn";
  if (normalized === "failed" || normalized === "exited" || normalized === "stopping") return "supervisor-pill-fail";
  return "supervisor-pill-neutral";
}

function processTooltip(proc: SupervisorProcessSnapshot): string {
  const parts: string[] = [];
  parts.push(`status: ${proc.status}`);
  if (proc.pid != null) parts.push(`pid ${proc.pid}`);
  if (proc.uptime_seconds != null) parts.push(`uptime ${formatUptime(proc.uptime_seconds)}`);
  if (proc.restarts > 0) parts.push(`${proc.restarts} restarts`);
  if (proc.last_exit_code != null) parts.push(`last exit ${proc.last_exit_code}`);
  if (proc.last_error) parts.push(proc.last_error);
  return parts.join(" · ");
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return `${h}h${m % 60}m`;
}
</script>

<style scoped>
.connection-cluster {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
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
  white-space: nowrap;
}

.stream-state.connected {
  background: var(--success-bg);
  color: var(--success-fg);
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
  .connection-cluster {
    align-items: flex-start;
  }

  .connection-summary {
    justify-content: flex-start;
    flex-wrap: nowrap;
  }

  .connection-cluster .service-url,
  .stream-state,
  .replica-summary,
  .supervisor-pills {
    display: none;
  }
}
</style>
