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
}
.connection-summary {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  flex-wrap: wrap;
}
.connection-cluster .service-url {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.replica-summary {
  color: var(--text-muted);
  font-size: 12px;
  white-space: nowrap;
}
.stream-state {
  white-space: nowrap;
}

.stream-state.connected {
  background: #dff5e7;
  color: #1f6f49;
}

.stream-state.connecting,
.stream-state.fallback {
  background: #fff2cc;
  color: #84620d;
}

.stream-state.disconnected {
  background: #eef2f6;
  color: #66717e;
}
.supervisor-pills {
  display: flex;
  align-items: center;
  gap: 4px;
}
.supervisor-pill {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 10px;
  border: 1px solid transparent;
  font-weight: 500;
}
.supervisor-pill-running {
  background: #dcfce7;
  color: #166534;
  border-color: #86efac;
}
.supervisor-pill-warn {
  background: #fef3c7;
  color: #92400e;
  border-color: #fcd34d;
}
.supervisor-pill-fail {
  background: #fee2e2;
  color: #991b1b;
  border-color: #fca5a5;
}
.supervisor-pill-neutral {
  background: #f1f5f9;
  color: #475569;
  border-color: #cbd5e1;
}
.supervisor-pill-stale {
  background: #f1f5f9;
  color: #94a3b8;
  border-color: #e2e8f0;
}
.supervisor-stale {
  font-size: 10px;
  color: #94a3b8;
  margin-left: 4px;
}
</style>
