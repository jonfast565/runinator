<template>
  <section class="grid gap-2 border-t border-border-subtle pt-3">
    <div class="flex flex-wrap items-baseline justify-between gap-2">
      <div>
        <h3 class="m-0 text-base font-semibold text-fg">{{ title }}</h3>
        <p class="m-0 mt-0.5 text-xs text-fg-muted">
          Engine-side broker traffic is captured automatically and correlated to this run.
        </p>
      </div>
      <button class="btn btn-sm" :disabled="loading" @click="refresh">
        <Icon name="refresh" />
        <span>Refresh</span>
      </button>
    </div>

    <p v-if="error" class="error m-0 text-xs">{{ error }}</p>
    <p v-else-if="loading && !records.length" class="m-0 text-xs text-fg-muted">
      Loading broker messages…
    </p>
    <p v-else-if="!records.length" class="m-0 text-xs text-fg-muted">
      No broker messages have reached the engine for this scope yet.
    </p>
    <div v-else class="grid gap-1.5">
      <details
        v-for="record in records"
        :key="String(record.id)"
        class="rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
      >
        <summary class="flex cursor-pointer flex-wrap items-center gap-x-2 gap-y-1 text-sm text-fg">
          <strong>{{ String(record.channel) }}</strong>
          <span class="badge">{{ String(record.direction) }}</span>
          <span class="text-xs text-fg-muted">{{ String(record.message_kind) }}</span>
          <span class="ml-auto text-xs text-fg-muted">{{
            formatDate(String(record.occurred_at))
          }}</span>
        </summary>
        <dl class="mt-2 grid grid-cols-[repeat(auto-fit,minmax(140px,1fr))] gap-1.5 text-xs">
          <div>
            <dt class="text-fg-muted">Delivery</dt>
            <dd class="m-0 break-all font-mono text-fg">{{ record.delivery_id || "—" }}</dd>
          </div>
          <div>
            <dt class="text-fg-muted">Dedupe key</dt>
            <dd class="m-0 break-all font-mono text-fg">{{ record.dedupe_key || "—" }}</dd>
          </div>
          <div>
            <dt class="text-fg-muted">Trace</dt>
            <dd class="m-0 break-all font-mono text-fg">{{ record.trace_id || "—" }}</dd>
          </div>
        </dl>
        <pre class="mt-2 max-h-72 overflow-auto rounded bg-surface p-2 text-xs">{{
          pretty(record.payload)
        }}</pre>
      </details>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listBrokerMessages } from "../../../core/api/commandCenterApi";
import type { JsonRecord } from "../../../core/domain/models";
import { formatDate, pretty } from "../../../core/utils/format";
import Icon from "./Icon.vue";

const props = withDefaults(
  defineProps<{
    workflowRunId?: string | null;
    pipelineRunId?: string | null;
    title?: string;
  }>(),
  { workflowRunId: null, pipelineRunId: null, title: "Broker messages" },
);

const records = ref<JsonRecord[]>([]);
const loading = ref(false);
const error = ref("");
let pollTimer = 0;

async function refresh(): Promise<void> {
  if (loading.value) {
    return;
  }

  loading.value = true;
  error.value = "";

  try {
    records.value = await listBrokerMessages({
      workflowRunId: props.workflowRunId ?? undefined,
      pipelineRunId: props.pipelineRunId ?? undefined,
      limit: 100,
    });
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

watch(
  () => [props.workflowRunId, props.pipelineRunId],
  () => void refresh(),
  { immediate: true },
);

onMounted(() => {
  pollTimer = window.setInterval(() => void refresh(), 1500);
});

onBeforeUnmount(() => {
  window.clearInterval(pollTimer);
});
</script>
