<template>
  <section class="pane replicas-pane">
    <LocalWorkerPanel />
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

        <div v-if="!filteredReplicas.length" class="empty-state">
          No replicas match the current filters.
        </div>

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
              <span class="replica-row-name">{{
                replica.display_name || replica.host || replica.instance_id
              }}</span>
              <span class="replica-row-status" :class="`replica-row-status-${replica.status}`">{{
                replica.status
              }}</span>
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
              <h2>
                {{
                  selectedReplica.display_name ||
                  selectedReplica.host ||
                  selectedReplica.instance_id
                }}
              </h2>
              <p class="replicas-detail-subtitle">
                {{ replicaKindLabel(selectedReplica.replica_type) }} · runtime
                {{ selectedReplica.runtime_id }}
              </p>
            </div>
            <span
              class="replica-status-badge"
              :class="`replica-status-badge-${selectedReplica.status}`"
            >
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
            <div class="replicas-section-head">
              <h3>Telemetry</h3>
              <span class="replicas-section-hint">
                {{ samplesLoading ? "loading…" : `${samples.length} sample(s), last hour` }}
              </span>
            </div>
            <div class="sparkline-grid">
              <Sparkline
                label="CPU"
                :values="cpuSeries"
                :max="100"
                unit="%"
                color="var(--accent)"
              />
              <Sparkline label="Memory" :values="memSeries" :max="100" unit="%" color="#7c5cff" />
              <Sparkline
                label="Process CPU"
                :values="procCpuSeries"
                :max="null"
                unit="%"
                color="#0ea5a5"
              />
              <Sparkline label="Load (1m)" :values="loadSeries" :max="null" color="#f59e0b" />
              <Sparkline
                label="Net In"
                :values="rxSeries"
                :max="null"
                color="#22c55e"
                :format="formatRate"
              />
              <Sparkline
                label="Net Out"
                :values="txSeries"
                :max="null"
                color="#ef4444"
                :format="formatRate"
              />
            </div>
          </div>

          <div class="replicas-section">
            <h3>Attributes</h3>
            <JsonEditor
              class="replicas-attributes"
              :model-value="pretty(selectedReplica.attributes ?? {})"
              readonly
              title=""
            />
          </div>
        </template>

        <div v-else class="empty-state">
          Select a replica to inspect its health, address, and runtime details.
        </div>
      </section>
    </div>
    <NodePoolsPanel />
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import LocalWorkerPanel from "../components/shared/LocalWorkerPanel.vue";
import NodePoolsPanel from "../components/shared/NodePoolsPanel.vue";
import Sparkline from "../components/shared/Sparkline.vue";
import { fetchReplicaSamples, type ReplicaSample } from "../../api/commandCenterApi";
import { useAppStore } from "../../stores/app";
import type { ReplicaKind } from "../../types/models";
import { formatDate, pretty } from "../../utils/format";

const app = useAppStore();
const loading = ref(false);
const selectedReplicaId = ref<string | null>(null);
const samples = ref<ReplicaSample[]>([]);
const samplesLoading = ref(false);

const cpuSeries = computed(() => samples.value.map((sample) => sample.cpu_percent));
const memSeries = computed(() => samples.value.map((sample) => sample.mem_percent));
const loadSeries = computed(() => samples.value.map((sample) => sample.load_one ?? 0));
const rxSeries = computed(() => samples.value.map((sample) => sample.net_rx_bytes_per_sec));
const txSeries = computed(() => samples.value.map((sample) => sample.net_tx_bytes_per_sec));
const procCpuSeries = computed(() => samples.value.map((sample) => sample.process_cpu_percent));

function formatRate(bytesPerSec: number): string {
  if (!Number.isFinite(bytesPerSec) || bytesPerSec <= 0) {
    return "0 B/s";
  }

  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let value = bytesPerSec;
  let unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${value < 10 && unit > 0 ? value.toFixed(1) : String(Math.round(value))} ${units[unit]}`;
}

async function loadSamples(replicaId: string | null) {
  if (!replicaId) {
    samples.value = [];
    return;
  }

  samplesLoading.value = true;

  try {
    const series = await fetchReplicaSamples(replicaId);
    samples.value = series.samples;
  } catch {
    samples.value = [];
  } finally {
    samplesLoading.value = false;
  }
}

const filteredReplicas = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return app.replicas;
  }

  return app.replicas.filter((replica) => {
    const haystack = [
      replica.display_name,
      replica.host,
      replica.instance_id,
      replica.runtime_id,
      replica.observed_ip,
      replica.replica_type,
      replica.status,
      replica.replica_id,
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(query);
  });
});

const selectedReplica = computed(() => {
  if (selectedReplicaId.value == null) {
    return filteredReplicas.value[0] ?? null;
  }

  return (
    filteredReplicas.value.find((replica) => replica.replica_id === selectedReplicaId.value) ??
    filteredReplicas.value[0]
  );
});

const staleCount = computed(
  () => app.replicas.filter((replica) => replica.status === "stale").length,
);
const offlineCount = computed(
  () => app.replicas.filter((replica) => replica.status === "offline").length,
);

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
    case "postgres":
      return "Postgres";
    case "archiver":
      return "Archiver";
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

// reload the telemetry time-series whenever the inspected replica changes.
watch(
  () => selectedReplica.value.replica_id,
  (replicaId) => {
    void loadSamples(replicaId);
  },
  { immediate: true },
);

onMounted(async () => {
  if (!app.replicas.length) {
    await refresh();
  }

  if (!selectedReplicaId.value && filteredReplicas.value.length) {
    selectedReplicaId.value = filteredReplicas.value[0].replica_id;
  }
});
</script>

<style scoped>
.replicas-pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: hidden;
}

.replicas-layout {
  display: grid;
  flex: 1 1 auto;
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
  flex: 1 1 auto;
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
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

.replicas-section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}

.replicas-section-hint {
  color: var(--text-muted);
  font-size: 12px;
}

.sparkline-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
}

.replicas-attributes {
  max-height: 360px;
}

@media (max-width: 900px) {
  .replicas-pane {
    overflow: auto;
  }

  .replicas-layout {
    flex: 0 0 auto;
    grid-template-columns: 1fr;
  }

  .replica-list {
    max-height: 340px;
  }

  .replicas-grid {
    grid-template-columns: 1fr;
  }
}
</style>
