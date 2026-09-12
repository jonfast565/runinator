<template>
  <section class="flex min-h-0 flex-col gap-3 overflow-auto p-2">
    <div class="flex flex-wrap items-end gap-3">
      <label v-if="!adapterId" class="grid gap-1"
        >Adapter
        <select v-model="selectedId" class="input">
          <option value="">Select an adapter</option>
          <option v-for="adapter in adapters" :key="adapter.id" :value="adapter.id">
            {{ adapter.name }}
          </option>
        </select>
      </label>
      <label class="grid gap-1"
        >Incoming deliveries
        <select v-model="mode" class="input" :disabled="!currentId || busy" @change="saveMode">
          <option value="disabled">Observe and admit</option>
          <option value="paused">Pause and queue in arrival order</option>
          <option value="review">Hold for review</option>
        </select>
      </label>
      <button class="btn" :disabled="!currentId || busy" @click="refresh">Refresh</button>
      <button
        v-if="mode === 'paused' && pausedCount"
        class="btn"
        :disabled="busy"
        @click="releasePaused"
      >
        Release queued ({{ pausedCount }})
      </button>
      <span class="text-sm text-fg-muted"
        >Completed history follows the server's adapter diagnostic retention setting.</span
      >
    </div>
    <p v-if="error" role="alert" class="text-danger">{{ error }}</p>
    <template v-if="currentId">
      <h3>Poll attempts and test jobs</h3>
      <p v-if="!attempts.length" class="text-sm text-fg-muted">No poll attempts recorded.</p>
      <details
        v-for="attempt in attempts"
        :key="attempt.id"
        class="rounded border border-border-subtle p-2"
      >
        <summary>
          {{ formatDate(attempt.created_at) }} · {{ attempt.dry_run ? "Test" : "Poll" }} ·
          {{ attempt.state }}
        </summary>
        <p v-if="attempt.error" class="text-danger">{{ attempt.error }}</p>
        <p class="text-xs">
          Attempt {{ attempt.id }} · revision {{ attempt.adapter_revision }} · deadline
          {{ formatDate(attempt.deadline_at) }}
        </p>
        <pre v-if="attempt.result" class="max-h-72 overflow-auto text-xs">{{
          pretty(attempt.result)
        }}</pre>
      </details>
      <h3>Deliveries</h3>
      <p v-if="!deliveries.length" class="text-sm text-fg-muted">
        No deliveries yet. New verified events and rejected deliveries will appear here.
      </p>
      <article
        v-for="record in deliveries"
        :key="record.id"
        class="grid gap-2 rounded border border-border-subtle p-3"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <strong>{{ record.event?.event_type || "Rejected request" }} · {{ record.state }}</strong>
          <span class="text-xs text-fg-muted"
            >{{ formatDate(record.received_at) }} · revision {{ record.origin.revision }}</span
          >
        </div>
        <p class="m-0 break-all text-sm">
          {{ record.event?.scope }} · {{ record.event?.correlation_key }}
        </p>
        <p v-if="record.error" class="m-0 text-danger">{{ record.error }}</p>
        <div class="flex flex-wrap gap-2">
          <template
            v-if="
              ['held', 'failed', 'rejected'].includes(record.state) &&
              !(record.state === 'held' && record.hold_mode === 'paused')
            "
          >
            <button
              v-if="record.event"
              class="btn btn-sm"
              :disabled="busy"
              @click="decide(record, record.state === 'held' ? 'approve' : 'retry')"
            >
              {{ record.state === "held" ? "Approve delivery" : "Retry delivery" }}
            </button>
            <button class="btn btn-sm" :disabled="busy" @click="decide(record, 'drop')">
              Drop
            </button>
          </template>
          <button v-if="bindingId(record)" class="btn btn-sm" @click="openBinding(record)">
            Open orchestration
          </button>
          <span v-if="record.state === 'held_at_pipeline'" class="text-sm"
            >Awaiting approval in External Events.</span
          >
        </div>
        <details>
          <summary>Route and intent preview</summary>
          <pre class="max-h-80 overflow-auto text-xs">{{ pretty(record.preview) }}</pre>
        </details>
        <details>
          <summary>Normalized event and admission result</summary>
          <pre class="max-h-80 overflow-auto text-xs">{{
            pretty({ event: record.event, outcome: record.outcome, attempt_id: record.attempt_id })
          }}</pre>
        </details>
      </article>
      <BrokerMessageLog :adapter-id="currentId" title="Adapter broker messages" />
    </template>
  </section>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { AdapterDefinition } from "../../../core/domain/models";
import type {
  AdapterDeliveryRecord,
  AdapterPollAttempt,
} from "../../../core/domain/models/orchestration/orchestration";
import { fetchAdapters } from "../../../core/services/orchestrations";
import {
  fetchAdapterDeliveries,
  fetchAdapterAttempts,
  fetchAdapterInspection,
  setAdapterInspection,
  decideAdapterDelivery,
  releasePausedAdapterDeliveries,
} from "../../../core/api/commandCenterApi";
import { formatDate, pretty } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import { useOrchestrationsStore } from "../../adapters/pinia/orchestrations";
import BrokerMessageLog from "../shared/BrokerMessageLog.vue";
const props = defineProps<{ adapterId?: string }>();
const emit = defineEmits<{ "open-binding": [] }>();
const app = useAppStore();
const orchestrations = useOrchestrationsStore();
const selectedId = ref("");
const currentId = computed(() => props.adapterId ?? selectedId.value);
const adapters = ref<AdapterDefinition[]>([]);
const deliveries = ref<AdapterDeliveryRecord[]>([]);
const attempts = ref<AdapterPollAttempt[]>([]);
const mode = ref<"disabled" | "paused" | "review">("disabled");
const error = ref("");
const busy = ref(false);
const pausedCount = computed(
  () =>
    deliveries.value.filter((record) => record.state === "held" && record.hold_mode === "paused")
      .length,
);
let timer = 0;

async function refresh() {
  const id = currentId.value;

  if (!id) {
    return;
  }

  try {
    const values = await Promise.all([
      fetchAdapterDeliveries(id),
      fetchAdapterAttempts(id),
      fetchAdapterInspection(id),
    ]);

    if (id !== currentId.value) {
      return;
    }

    deliveries.value = values[0];
    attempts.value = values[1];
    mode.value = values[2].mode;
    error.value = "";
  } catch (cause) {
    error.value = String(cause);
  }
}

async function mutate(operation: () => Promise<unknown>) {
  busy.value = true;

  try {
    await operation();
    await refresh();
  } catch (cause) {
    error.value = String(cause);
  } finally {
    busy.value = false;
  }
}

function saveMode() {
  return mutate(() => setAdapterInspection(currentId.value, mode.value));
}

function decide(record: AdapterDeliveryRecord, decision: "approve" | "retry" | "drop") {
  return mutate(() => decideAdapterDelivery(currentId.value, record.id, decision));
}

function releasePaused() {
  return mutate(() => releasePausedAdapterDeliveries(currentId.value));
}

function bindingId(record: AdapterDeliveryRecord) {
  return typeof record.outcome?.orchestration_binding_id === "string"
    ? record.outcome.orchestration_binding_id
    : null;
}

async function openBinding(record: AdapterDeliveryRecord) {
  const id = bindingId(record);

  if (id) {
    await orchestrations.select(id);
    app.activeTab = "Orchestrations";
    emit("open-binding");
  }
}

watch(
  currentId,
  () => {
    deliveries.value = [];
    attempts.value = [];
    void refresh();
  },
  { immediate: true },
);
onMounted(async () => {
  try {
    adapters.value = await fetchAdapters();
  } catch (cause) {
    error.value = String(cause);
  }

  timer = window.setInterval(() => void refresh(), 2000);
});
onBeforeUnmount(() => {
  window.clearInterval(timer);
});
</script>
