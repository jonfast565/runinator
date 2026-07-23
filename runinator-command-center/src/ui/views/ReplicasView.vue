<template>
  <section class="pane flex h-full flex-col gap-2.5 overflow-hidden max-md:overflow-auto">
    <div
      class="grid min-h-0 flex-1 gap-2.5 grid-cols-1 md:grid-cols-[minmax(260px,320px)_minmax(0,1fr)] max-md:flex-none"
    >
      <aside class="panel flex min-h-0 flex-col">
        <div class="panel-toolbar">
          <h2 class="m-0 text-base font-semibold text-fg">Replicas</h2>
          <button class="btn" :disabled="loadingReplicas" @click="refresh">
            <LoadingSpinner v-if="loadingReplicas" size="sm" label="Refreshing replicas" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </div>

        <div class="mb-2 flex flex-wrap gap-1.5">
          <span class="rounded-pill bg-surface-subtle px-2 py-0.5 text-xs text-fg-subtle"
            >{{ app.liveReplicaCount }} live</span
          >
          <span class="rounded-pill bg-surface-subtle px-2 py-0.5 text-xs text-fg-subtle"
            >{{ staleCount }} stale</span
          >
          <span class="rounded-pill bg-surface-subtle px-2 py-0.5 text-xs text-fg-subtle"
            >{{ offlineCount }} offline</span
          >
        </div>

        <LoadingPanel
          v-if="loadingReplicas && !app.replicas.length"
          compact
          :message="loadingReplicasMessage || 'Loading replicas…'"
        />
        <div v-else-if="!filteredReplicas.length" class="py-3.5 text-fg-muted">
          No replicas match the current filters.
        </div>

        <div v-else class="flex min-h-0 flex-1 flex-col gap-1.5 overflow-auto max-md:max-h-[340px]">
          <button
            v-for="replica in filteredReplicas"
            :key="replica.replica_id"
            type="button"
            class="w-full rounded-lg border border-border bg-surface p-2.5 text-left"
            :class="
              selectedReplica?.replica_id === replica.replica_id
                ? 'border-accent bg-accent-soft'
                : ''
            "
            @click="selectedReplicaId = replica.replica_id"
          >
            <div class="mb-1 flex items-center justify-between gap-2">
              <span class="min-w-0 truncate font-semibold">{{
                replica.display_name || replica.host || replica.instance_id
              }}</span>
              <span
                class="rounded-pill px-2 py-0.5 text-xs font-semibold capitalize"
                :class="{
                  'bg-success-bg text-success-fg': replica.status === 'live',
                  'bg-warning-bg text-warning-fg': replica.status === 'stale',
                  'bg-danger-bg text-danger-fg': replica.status === 'offline',
                }"
                >{{ replica.status }}</span
              >
            </div>
            <div class="flex flex-wrap items-center justify-start gap-2 text-xs text-fg-muted">
              <span>{{ replicaKindLabel(replica.replica_type) }}</span>
              <span>{{ replica.observed_ip || replica.host || "ip unknown" }}</span>
              <span>#{{ replica.replica_id }}</span>
            </div>
          </button>
        </div>
      </aside>

      <section class="panel flex min-h-0 flex-col overflow-auto">
        <template v-if="selectedReplica">
          <div class="mb-3.5 flex items-start justify-between gap-3">
            <div>
              <h2 class="m-0 mb-1 text-base font-semibold text-fg">
                {{
                  selectedReplica.display_name ||
                  selectedReplica.host ||
                  selectedReplica.instance_id
                }}
              </h2>
              <p class="m-0 text-fg-muted">
                {{ replicaKindLabel(selectedReplica.replica_type) }} · runtime
                {{ selectedReplica.runtime_id }}
              </p>
            </div>
            <span
              class="rounded-pill px-2 py-0.5 text-xs font-semibold capitalize"
              :class="{
                'bg-success-bg text-success-fg': selectedReplica.status === 'live',
                'bg-warning-bg text-warning-fg': selectedReplica.status === 'stale',
                'bg-danger-bg text-danger-fg': selectedReplica.status === 'offline',
              }"
            >
              {{ selectedReplica.status }}
            </span>
          </div>

          <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Replica ID</label
              >
              <div>{{ selectedReplica.replica_id }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Observed IP</label
              >
              <div class="font-mono">{{ selectedReplica.observed_ip || "-" }}</div>
            </div>
            <div>
              <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase">Host</label>
              <div>{{ selectedReplica.host || "-" }}</div>
            </div>
            <div>
              <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase">Port</label>
              <div>{{ selectedReplica.port ?? "-" }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Base Path</label
              >
              <div class="font-mono">{{ selectedReplica.base_path || "/" }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Instance ID</label
              >
              <div class="font-mono">{{ selectedReplica.instance_id }}</div>
            </div>
            <div>
              <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Version</label
              >
              <div class="font-mono">{{ selectedReplica.version || "-" }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >First Seen</label
              >
              <div>{{ formatDate(selectedReplica.first_seen_at) }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Last Heartbeat</label
              >
              <div>{{ formatDate(selectedReplica.last_heartbeat_at) }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Last Seen</label
              >
              <div>{{ formatDate(selectedReplica.last_seen_at) }}</div>
            </div>
            <div>
              <label
                class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                >Offline At</label
              >
              <div>{{ formatDate(selectedReplica.offline_at) }}</div>
            </div>
          </div>

          <div class="mt-[18px]">
            <div class="mb-2 flex items-baseline justify-between gap-2">
              <h3 class="m-0 text-sm font-semibold text-fg">Telemetry</h3>
              <span class="text-xs text-fg-muted">
                <LoadingSpinner v-if="samplesLoading" size="sm" label="Loading telemetry" />
                {{
                  samplesLoading
                    ? "Loading telemetry…"
                    : `${samples.length} sample(s), last hour`
                }}
              </span>
            </div>
            <div class="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-2.5">
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

          <div class="mt-[18px]">
            <h3 class="m-0 mb-2 text-sm font-semibold text-fg">Attributes</h3>
            <JsonEditor
              class="max-h-[360px]"
              :model-value="pretty(selectedReplica.attributes ?? {})"
              readonly
              title=""
            />
          </div>
        </template>

        <div v-else class="py-3.5 text-fg-muted">
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
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import NodePoolsPanel from "../components/shared/NodePoolsPanel.vue";
import Sparkline from "../components/shared/Sparkline.vue";
import { replicaSamplesService } from "../../core/services";
import type { ReplicaSample } from "../../core/services";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOperationLoading } from "../composables/useOperationLoading";
import type { ReplicaKind } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";

const app = useAppStore();
const { isLoading: loadingReplicas, loadingMessage: loadingReplicasMessage } =
  useOperationLoading(["Loading replicas", "Loading replica samples"]);
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
    const series = await replicaSamplesService.fetch(replicaId);
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
  await app.runOperation("Loading replicas", () => app.refreshReplicas());
}

function replicaKindLabel(kind: ReplicaKind): string {
  switch (kind) {
    case "webservice":
      return "Web Service";
    case "worker":
      return "Worker";
    case "waker":
      return "Waker";
    case "background":
      return "Background Worker";
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

