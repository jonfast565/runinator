<template>
  <section class="pane replicas-pane">
    <div class="replicas-layout">
      <aside class="panel replicas-list-panel">
        <div class="panel-toolbar">
          <h2>Replicas</h2>
          <button class="btn" :disabled="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
        </div>

        <div class="replica-list-summary">
          <span class="replica-stat">{{ app.liveReplicaCount }} live</span>
          <span class="replica-stat">{{ staleCount }} stale</span>
          <span class="replica-stat">{{ offlineCount }} offline</span>
        </div>

        <div v-if="!filteredReplicas.length" class="empty-state">No replicas match the current filters.</div>

        <div v-else class="replica-list">
          <button
            v-for="replica in filteredReplicas"
            :key="replica.replica_id"
            type="button"
            class="replica-row"
            :class="{ selected: selectedReplica?.replica_id === replica.replica_id }"
            @click="selectedReplicaId = replica.replica_id"
          >
            <div class="replica-row-top">
              <span class="replica-row-name">{{ replica.display_name || replica.host || replica.instance_id }}</span>
              <span class="replica-row-status" :class="`replica-row-status-${replica.status}`">{{ replica.status }}</span>
            </div>
            <div class="replica-row-meta">
              <span>{{ replicaKindLabel(replica.replica_type) }}</span>
              <span>{{ replica.observed_ip || replica.host || "ip unknown" }}</span>
              <span>#{{ replica.replica_id }}</span>
            </div>
          </button>
        </div>
      </aside>

      <section class="panel replicas-detail-panel">
        <template v-if="selectedReplica">
          <div class="replicas-detail-head">
            <div>
              <h2>{{ selectedReplica.display_name || selectedReplica.host || selectedReplica.instance_id }}</h2>
              <p class="replicas-detail-subtitle">
                {{ replicaKindLabel(selectedReplica.replica_type) }} · runtime {{ selectedReplica.runtime_id }}
              </p>
            </div>
            <span class="replica-status-badge" :class="`replica-status-badge-${selectedReplica.status}`">
              {{ selectedReplica.status }}
            </span>
          </div>

          <div class="replicas-grid">
            <div class="replicas-field">
              <label>Replica ID</label>
              <div>{{ selectedReplica.replica_id }}</div>
            </div>
            <div class="replicas-field">
              <label>Observed IP</label>
              <div class="mono">{{ selectedReplica.observed_ip || "-" }}</div>
            </div>
            <div class="replicas-field">
              <label>Host</label>
              <div>{{ selectedReplica.host || "-" }}</div>
            </div>
            <div class="replicas-field">
              <label>Port</label>
              <div>{{ selectedReplica.port ?? "-" }}</div>
            </div>
            <div class="replicas-field">
              <label>Base Path</label>
              <div class="mono">{{ selectedReplica.base_path || "/" }}</div>
            </div>
            <div class="replicas-field">
              <label>Instance ID</label>
              <div class="mono">{{ selectedReplica.instance_id }}</div>
            </div>
            <div class="replicas-field">
              <label>Version</label>
              <div class="mono">{{ selectedReplica.version || "-" }}</div>
            </div>
            <div class="replicas-field">
              <label>First Seen</label>
              <div>{{ formatDate(selectedReplica.first_seen_at) }}</div>
            </div>
            <div class="replicas-field">
              <label>Last Heartbeat</label>
              <div>{{ formatDate(selectedReplica.last_heartbeat_at) }}</div>
            </div>
            <div class="replicas-field">
              <label>Last Seen</label>
              <div>{{ formatDate(selectedReplica.last_seen_at) }}</div>
            </div>
            <div class="replicas-field">
              <label>Offline At</label>
              <div>{{ formatDate(selectedReplica.offline_at) }}</div>
            </div>
          </div>

          <div class="replicas-section">
            <h3>Attributes</h3>
            <pre class="replicas-pre">{{ pretty(selectedReplica.attributes ?? {}) }}</pre>
          </div>
        </template>

        <div v-else class="empty-state">Select a replica to inspect its health, address, and runtime details.</div>
      </section>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import { useAppStore } from "../stores/app";
import type { ReplicaKind } from "../types/models";
import { formatDate, pretty } from "../utils/format";

const app = useAppStore();
const loading = ref(false);
const selectedReplicaId = ref<string | null>(null);

const filteredReplicas = computed(() => {
  const query = app.normalizedSearch;
  if (!query) return app.replicas;
  return app.replicas.filter((replica) => {
    const haystack = [
      replica.display_name,
      replica.host,
      replica.instance_id,
      replica.runtime_id,
      replica.observed_ip,
      replica.replica_type,
      replica.status,
      String(replica.replica_id)
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(query);
  });
});

const selectedReplica = computed(() => {
  if (selectedReplicaId.value == null) return filteredReplicas.value[0] ?? null;
  return filteredReplicas.value.find((replica) => replica.replica_id === selectedReplicaId.value) ?? filteredReplicas.value[0] ?? null;
});

const staleCount = computed(() => app.replicas.filter((replica) => replica.status === "stale").length);
const offlineCount = computed(() => app.replicas.filter((replica) => replica.status === "offline").length);

async function refresh() {
  loading.value = true;
  try {
    await app.runOperation("Loading replicas", () => app.refreshReplicas());
  } finally {
    loading.value = false;
  }
}

function replicaKindLabel(kind: ReplicaKind): string {
  switch (kind) {
    case "webservice":
      return "Web Service";
    case "worker":
      return "Worker";
    case "waker":
      return "Waker";
  }
}

watch(filteredReplicas, (replicas) => {
  if (!replicas.length) {
    selectedReplicaId.value = null;
    return;
  }
  if (!replicas.some((replica) => replica.replica_id === selectedReplicaId.value)) {
    selectedReplicaId.value = replicas[0].replica_id;
  }
});

onMounted(async () => {
  if (!app.replicas.length) await refresh();
  if (!selectedReplicaId.value && filteredReplicas.value.length) {
    selectedReplicaId.value = filteredReplicas.value[0].replica_id;
  }
});
</script>

<style scoped>
.replicas-pane {
  overflow: hidden;
}

.replicas-layout {
  display: grid;
  height: 100%;
  min-height: 0;
  gap: 10px;
  grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
}

.replicas-list-panel,
.replicas-detail-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.replica-list-summary {
  display: flex;
  gap: 6px;
  margin-bottom: 8px;
  flex-wrap: wrap;
}

.replica-stat {
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  color: var(--text-subtle);
  padding: 3px 8px;
  font-size: 12px;
}

.replica-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow: auto;
  min-height: 0;
}

.replica-row {
  width: 100%;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface);
  padding: 10px;
  text-align: left;
}

.replica-row.selected {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.replica-row-top,
.replica-row-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.replica-row-top {
  margin-bottom: 4px;
}

.replica-row-name {
  font-weight: 600;
}

.replica-row-meta {
  color: var(--text-muted);
  font-size: 12px;
  justify-content: flex-start;
  flex-wrap: wrap;
}

.replica-row-status,
.replica-status-badge {
  border-radius: var(--radius-pill);
  padding: 2px 8px;
  font-size: 12px;
  font-weight: 600;
  text-transform: capitalize;
}

.replica-row-status-live,
.replica-status-badge-live {
  background: var(--success-bg);
  color: var(--success-fg);
}

.replica-row-status-stale,
.replica-status-badge-stale {
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.replica-row-status-offline,
.replica-status-badge-offline {
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.replicas-detail-panel {
  overflow: auto;
}

.replicas-detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.replicas-detail-head h2 {
  margin: 0 0 4px;
}

.replicas-detail-subtitle {
  margin: 0;
  color: var(--text-muted);
}

.replicas-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.replicas-field label {
  display: block;
  margin-bottom: 4px;
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.mono {
  font-family: var(--font-mono);
}

.replicas-section {
  margin-top: 18px;
}

.replicas-section h3 {
  margin: 0 0 8px;
}

.replicas-pre {
  margin: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface-sunken);
  padding: 12px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.45;
}

@media (max-width: 900px) {
  .replicas-layout {
    grid-template-columns: 1fr;
  }

  .replicas-grid {
    grid-template-columns: 1fr;
  }
}
</style>
