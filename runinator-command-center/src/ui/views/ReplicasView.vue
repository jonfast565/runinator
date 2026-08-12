<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      orientation="vertical"
      storage-key="command-center.replicas.vertical-split"
      :initial-first-pct="68"
      :min-first="360"
      :min-second="180"
      :second-enabled="canManageAgent"
      collapsible-second
      second-label="Node pools"
      second-icon="grid"
    >
      <template #first>
        <SplitPane
          class="h-full w-full"
          storage-key="command-center.replicas.horizontal-split"
          :initial-first-pct="28"
          :min-first="280"
          :min-second="520"
          collapsible-first
          first-label="Replicas"
          first-icon="list"
          mobile-mode="toggle"
          :mobile-detail-active="selectedReplicaId != null"
        >
          <template #first>
            <aside class="panel flex min-h-0 flex-col">
              <div class="panel-toolbar">
                <h2 class="m-0 text-base font-semibold text-fg">Replicas</h2>
                <div class="flex gap-1.5">
                  <button v-if="canEnrollAgents" class="btn" @click="openEnrollment">
                    <Icon name="plus" />
                    <span>Enroll</span>
                  </button>
                  <button class="btn" :disabled="loadingReplicas" @click="refresh">
                    <LoadingSpinner v-if="loadingReplicas" size="sm" label="Refreshing replicas" />
                    <Icon v-else name="refresh" />
                    <span>Refresh</span>
                  </button>
                </div>
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

              <div
                v-else
                class="flex min-h-0 flex-1 flex-col gap-1.5 overflow-auto max-md:max-h-[340px]"
              >
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
                        'bg-success-bg text-success-fg':
                          replica.status === 'live' && !isAgentDegraded(replica),
                        'bg-warning-bg text-warning-fg':
                          replica.status === 'stale' || isAgentDegraded(replica),
                        'bg-danger-bg text-danger-fg': replica.status === 'offline',
                      }"
                      >{{ replicaDisplayStatus(replica) }}</span
                    >
                  </div>
                  <div
                    class="flex flex-wrap items-center justify-start gap-2 text-xs text-fg-muted"
                  >
                    <span>{{ replicaKindLabel(replica.replica_type) }}</span>
                    <span>{{ replica.observed_ip || replica.host || "ip unknown" }}</span>
                    <span>#{{ replica.replica_id }}</span>
                  </div>
                </button>
              </div>
            </aside>
          </template>

          <template #second>
            <section class="panel flex min-h-0 flex-col overflow-hidden">
              <MobileBackBar label="Back to replicas" @back="selectedReplicaId = null" />
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

                <div class="min-h-0 flex-1 overflow-auto pr-1">
                  <div
                    v-if="selectedAgentStatus"
                    class="mb-4 rounded-lg border p-3"
                    :class="
                      selectedAgentStatus.connection_state === 'connected'
                        ? 'border-border bg-surface-subtle'
                        : 'border-warning-border bg-warning-bg'
                    "
                  >
                    <div class="mb-2 flex items-center justify-between gap-2">
                      <h3 class="m-0 text-sm font-semibold text-fg">Remote agent</h3>
                      <span class="text-xs font-semibold capitalize">
                        {{ selectedAgentStatus.connection_state.replaceAll("_", " ") }}
                      </span>
                    </div>
                    <div class="grid grid-cols-2 gap-x-4 gap-y-2 text-sm md:grid-cols-4">
                      <div>
                        <span class="text-fg-muted">Broker</span><br />{{
                          selectedAgentStatus.broker_mode
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">In flight</span><br />{{
                          selectedAgentStatus.in_flight
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Completed</span><br />{{
                          selectedAgentStatus.succeeded
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Failed</span><br />{{
                          selectedAgentStatus.failed
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Providers</span><br />{{
                          selectedAgentStatus.provider_count
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Outbox</span><br />{{
                          selectedAgentStatus.outbox_depth
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Uptime</span><br />{{
                          formatDuration(selectedAgentStatus.uptime_seconds)
                        }}
                      </div>
                      <div>
                        <span class="text-fg-muted">Clock skew</span><br />{{
                          selectedAgentStatus.clock_skew_ms
                        }}
                        ms
                      </div>
                    </div>
                    <p
                      v-if="selectedAgentStatus.last_error"
                      class="mb-0 mt-2 text-sm text-danger-fg"
                    >
                      {{ selectedAgentStatus.last_error }}
                    </p>
                  </div>

                  <div v-if="selectedAgentStatus" class="mb-4 rounded-lg border border-border p-3">
                    <div class="mb-2 flex items-center justify-between gap-2">
                      <h3 class="m-0 text-sm font-semibold text-fg">Agent actions</h3>
                      <LoadingSpinner v-if="directiveBusy" size="sm" label="Running agent action" />
                    </div>
                    <div class="flex flex-wrap gap-2">
                      <button
                        v-if="canReadAgent"
                        class="btn"
                        :disabled="directiveBusy"
                        @click="issueDirective({ type: 'diagnostics' })"
                      >
                        Diagnostics
                      </button>
                      <button
                        v-if="canReadAgent"
                        class="btn"
                        :disabled="directiveBusy"
                        @click="issueDirective({ type: 'tail_logs', lines: 200 })"
                      >
                        Logs
                      </button>
                      <button
                        v-if="canManageAgent"
                        class="btn"
                        :disabled="directiveBusy"
                        @click="issueDirective({ type: 'drain' })"
                      >
                        Drain
                      </button>
                      <button
                        v-if="canManageAgent"
                        class="btn"
                        :disabled="directiveBusy"
                        @click="issueDirective({ type: 'undrain' })"
                      >
                        Undrain
                      </button>
                      <button
                        v-if="canManageAgent"
                        class="btn"
                        :disabled="directiveBusy"
                        @click="issueDirective({ type: 'restart' })"
                      >
                        Restart
                      </button>
                    </div>
                    <p v-if="directiveError" class="mb-0 mt-2 text-sm text-danger-fg">
                      {{ directiveError }}
                    </p>
                    <div v-if="agentDirectives.length" class="mt-3 flex flex-col gap-2">
                      <div
                        v-for="directive in agentDirectives.slice(0, 5)"
                        :key="directive.directive_id"
                        class="rounded border border-border bg-surface-subtle p-2 text-xs"
                      >
                        <div class="flex justify-between gap-2">
                          <span class="font-semibold">{{ directiveType(directive) }}</span>
                          <span class="capitalize">{{ directive.state }}</span>
                        </div>
                        <div v-if="directive.message" class="mt-1 text-danger-fg">
                          {{ directive.message }}
                        </div>
                        <pre
                          v-if="directive.payload != null"
                          class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap"
                          >{{ pretty(directive.payload) }}</pre>
                      </div>
                    </div>
                  </div>

                  <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Replica ID</label
                      >
                      <div>{{ selectedReplica.replica_id }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Observed IP</label
                      >
                      <div class="font-mono">{{ selectedReplica.observed_ip || "-" }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Host</label
                      >
                      <div>{{ selectedReplica.host || "-" }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Port</label
                      >
                      <div>{{ selectedReplica.port ?? "-" }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Base Path</label
                      >
                      <div class="font-mono">{{ selectedReplica.base_path || "/" }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
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
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >First Seen</label
                      >
                      <div>{{ formatDate(selectedReplica.first_seen_at) }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Last Heartbeat</label
                      >
                      <div>{{ formatDate(selectedReplica.last_heartbeat_at) }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
                        >Last Seen</label
                      >
                      <div>{{ formatDate(selectedReplica.last_seen_at) }}</div>
                    </div>
                    <div>
                      <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
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
                      <Sparkline
                        label="Memory used"
                        :values="memUsedSeries"
                        :max="memoryCapacity"
                        :format="formatBytes"
                        color="#7c5cff"
                      />
                      <Sparkline
                        label="Process CPU"
                        :values="procCpuSeries"
                        :max="null"
                        unit="%"
                        color="#0ea5a5"
                      />
                      <Sparkline
                        label="Process memory"
                        :values="procMemSeries"
                        :max="null"
                        :format="formatBytes"
                        color="#f59e0b"
                      />
                      <Sparkline
                        label="Load (1m)"
                        :values="loadSeries"
                        :max="null"
                        color="#a855f7"
                      />
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
                </div>
              </template>

              <div v-else class="py-3.5 text-fg-muted">
                Select a replica to inspect its health, address, and runtime details.
              </div>
            </section>
          </template>
        </SplitPane>
      </template>
      <template #second>
        <NodePoolsPanel />
      </template>
    </SplitPane>
    <Modal v-if="enrollmentOpen" title="Enroll a machine" width="620px" @close="closeEnrollment">
      <form class="flex flex-col gap-3" @submit.prevent="createEnrollmentToken">
        <template v-if="!createdEnrollmentToken">
          <p class="m-0 text-sm text-fg-muted">
            Create a short-lived, single-use token for a worker agent. Requested labels cannot
            exceed this allowed set.
          </p>
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium">Service URL</span>
            <input
              v-model.trim="enrollmentServiceUrl"
              class="input font-mono"
              required
              type="url"
            />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium">Lifetime (minutes)</span>
            <input
              v-model.number="enrollmentTtlMinutes"
              class="input"
              min="1"
              max="1440"
              required
              type="number"
            />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium">Allowed labels</span>
            <input
              v-model.trim="enrollmentLabels"
              class="input font-mono"
              placeholder="site=home, gpu=true"
            />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium">SPKI pin (optional)</span>
            <input
              v-model.trim="enrollmentSpkiPin"
              class="input font-mono"
              placeholder="base64 SHA-256 pin"
            />
          </label>
          <p v-if="enrollmentError" class="m-0 text-sm text-danger-fg">{{ enrollmentError }}</p>
          <div class="flex justify-end gap-2">
            <button type="button" class="btn" @click="closeEnrollment">Cancel</button>
            <button class="btn btn-primary" :disabled="creatingEnrollment" type="submit">
              <LoadingSpinner v-if="creatingEnrollment" size="sm" label="Creating token" />
              Create token
            </button>
          </div>
        </template>
        <template v-else>
          <div class="rounded-lg border border-warning-border bg-warning-bg p-3 text-sm">
            This token is shown once. Copy it now and pass it to the agent with
            <code>--enroll</code>.
          </div>
          <textarea
            class="input min-h-32 font-mono text-xs"
            readonly
            :value="createdEnrollmentToken"
          />
          <p v-if="enrollmentError" class="m-0 text-sm text-danger-fg">{{ enrollmentError }}</p>
          <div class="flex justify-end gap-2">
            <button type="button" class="btn" @click="copyEnrollmentToken">Copy token</button>
            <button type="button" class="btn btn-primary" @click="closeEnrollment">Done</button>
          </div>
        </template>
      </form>
    </Modal>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import Modal from "../components/shared/Modal.vue";
import NodePoolsPanel from "../components/shared/NodePoolsPanel.vue";
import Sparkline from "../components/shared/Sparkline.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import {
  agentDirectivesService,
  agentEnrollmentService,
  appService,
  replicaSamplesService,
} from "../../core/services";
import type { ReplicaSample } from "../../core/services";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useCapabilitiesStore } from "../../ui/adapters/pinia/capabilities";
import { useOperationLoading } from "../composables/useOperationLoading";
import type {
  AgentDirectiveKind,
  AgentDirectiveRecord,
  AgentStatusReport,
  ReplicaKind,
  ReplicaRecord,
} from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";

const app = useAppStore();
const capabilities = useCapabilitiesStore();
const canEnrollAgents = computed(() => capabilities.has("agents:enroll"));
const canReadAgent = computed(() => capabilities.has("audit:read"));
const canManageAgent = computed(() => capabilities.has("nodes:scale"));
const enrollmentOpen = ref(false);
const enrollmentServiceUrl = ref("");
const enrollmentTtlMinutes = ref(15);
const enrollmentLabels = ref("");
const enrollmentSpkiPin = ref("");
const enrollmentError = ref("");
const createdEnrollmentToken = ref("");
const creatingEnrollment = ref(false);
const { isLoading: loadingReplicas, loadingMessage: loadingReplicasMessage } = useOperationLoading([
  "Loading replicas",
  "Loading replica samples",
]);
const selectedReplicaId = ref<string | null>(null);
const samples = ref<ReplicaSample[]>([]);
const samplesLoading = ref(false);
const agentDirectives = ref<AgentDirectiveRecord[]>([]);
const directiveBusy = ref(false);
const directiveError = ref("");

async function loadDirectives(replicaId: string | null) {
  if (!replicaId || (!canReadAgent.value && !capabilities.has("secrets:read"))) {
    agentDirectives.value = [];
    return;
  }

  try {
    agentDirectives.value = await agentDirectivesService.list(replicaId);
  } catch {
    agentDirectives.value = [];
  }
}

async function issueDirective(kind: AgentDirectiveKind) {
  const replica = selectedReplica.value;

  if (!replica) {
    return;
  }

  directiveBusy.value = true;
  directiveError.value = "";

  try {
    await agentDirectivesService.issue(replica.replica_id, kind);
    await loadDirectives(replica.replica_id);
  } catch (error) {
    directiveError.value = error instanceof Error ? error.message : String(error);
  } finally {
    directiveBusy.value = false;
  }
}

function directiveType(directive: AgentDirectiveRecord): string {
  const type = (directive.kind as { type?: unknown }).type;
  return typeof type === "string" ? type.replaceAll("_", " ") : "directive";
}

const cpuSeries = computed(() => samples.value.map((sample) => sample.cpu_percent));
const memUsedSeries = computed(() => samples.value.map((sample) => sample.mem_used_bytes));
const memoryCapacity = computed(() =>
  Math.max(...samples.value.map((sample) => sample.mem_total_bytes), 0),
);
const loadSeries = computed(() => samples.value.map((sample) => sample.load_one ?? 0));
const rxSeries = computed(() => samples.value.map((sample) => sample.net_rx_bytes_per_sec));
const txSeries = computed(() => samples.value.map((sample) => sample.net_tx_bytes_per_sec));
const procCpuSeries = computed(() => samples.value.map((sample) => sample.process_cpu_percent));
const procMemSeries = computed(() => samples.value.map((sample) => sample.process_mem_bytes));

function formatBytes(bytes: number): string {
  return formatScaledBytes(bytes, false);
}

function formatRate(bytesPerSec: number): string {
  return formatScaledBytes(bytesPerSec, true);
}

function formatScaledBytes(bytes: number, perSecond: boolean): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return perSecond ? "0 B/s" : "0 B";
  }

  const suffix = perSecond ? "/s" : "";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${value < 10 && unit > 0 ? value.toFixed(1) : String(Math.round(value))} ${units[unit]}${suffix}`;
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

const selectedReplica = computed<ReplicaRecord | null>(() => {
  if (!filteredReplicas.value.length) {
    return null;
  }

  if (selectedReplicaId.value == null) {
    return filteredReplicas.value[0];
  }

  const index = filteredReplicas.value.findIndex(
    (replica) => replica.replica_id === selectedReplicaId.value,
  );
  return index >= 0 ? filteredReplicas.value[index] : filteredReplicas.value[0];
});

const selectedAgentStatus = computed(() => agentStatus(selectedReplica.value));

function agentStatus(replica: ReplicaRecord | null | undefined): AgentStatusReport | null {
  const status = replica?.attributes.status;

  if (!status || typeof status !== "object" || Array.isArray(status)) {
    return null;
  }

  if (typeof (status as { connection_state?: unknown }).connection_state !== "string") {
    return null;
  }

  return status as AgentStatusReport;
}

function isAgentDegraded(replica: ReplicaRecord): boolean {
  const status = agentStatus(replica);
  return replica.status === "live" && status != null && status.connection_state !== "connected";
}

function replicaDisplayStatus(replica: ReplicaRecord): string {
  return isAgentDegraded(replica) ? "degraded" : replica.status;
}

function formatDuration(seconds: number): string {
  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  if (seconds < 3600) {
    return `${String(Math.floor(seconds / 60))}m`;
  }

  return `${String(Math.floor(seconds / 3600))}h ${String(Math.floor((seconds % 3600) / 60))}m`;
}

const staleCount = computed(
  () => app.replicas.filter((replica) => replica.status === "stale").length,
);
const offlineCount = computed(
  () => app.replicas.filter((replica) => replica.status === "offline").length,
);

async function refresh() {
  await app.runOperation("Loading replicas", () => app.refreshReplicas());
}

function openEnrollment() {
  enrollmentServiceUrl.value = appService.getState().serviceUrl ?? "";
  enrollmentTtlMinutes.value = 15;
  enrollmentLabels.value = "";
  enrollmentSpkiPin.value = "";
  enrollmentError.value = "";
  createdEnrollmentToken.value = "";
  enrollmentOpen.value = true;
}

function closeEnrollment() {
  createdEnrollmentToken.value = "";
  enrollmentOpen.value = false;
}

function parseEnrollmentLabels(): Record<string, string> {
  const labels: Record<string, string> = {};

  for (const entry of enrollmentLabels.value
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean)) {
    const separator = entry.indexOf("=");

    if (separator <= 0 || separator === entry.length - 1) {
      throw new Error(`Invalid label '${entry}'; expected KEY=VALUE.`);
    }

    labels[entry.slice(0, separator).trim()] = entry.slice(separator + 1).trim();
  }

  return labels;
}

async function createEnrollmentToken() {
  enrollmentError.value = "";
  creatingEnrollment.value = true;

  try {
    const response = await agentEnrollmentService.create({
      ttl_seconds: Math.round(enrollmentTtlMinutes.value * 60),
      labels: parseEnrollmentLabels(),
      service_url: enrollmentServiceUrl.value,
      spki_pin: enrollmentSpkiPin.value || null,
    });
    createdEnrollmentToken.value = response.token;
  } catch (error) {
    enrollmentError.value = error instanceof Error ? error.message : String(error);
  } finally {
    creatingEnrollment.value = false;
  }
}

async function copyEnrollmentToken() {
  try {
    await navigator.clipboard.writeText(createdEnrollmentToken.value);
    enrollmentError.value = "";
  } catch {
    enrollmentError.value = "Clipboard access failed. Select and copy the token manually.";
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

  if (
    selectedReplicaId.value != null &&
    !replicas.some((replica) => replica.replica_id === selectedReplicaId.value)
  ) {
    selectedReplicaId.value = null;
  }
});

// reload the telemetry time-series whenever the inspected replica changes.
watch(
  () => selectedReplica.value?.replica_id ?? null,
  (replicaId) => {
    void loadSamples(replicaId);
    void loadDirectives(replicaId);
  },
  { immediate: true },
);

onMounted(async () => {
  if (!app.replicas.length) {
    await refresh();
  }
});
</script>
