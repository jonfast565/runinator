<template>
  <section class="grid gap-2 border-t border-border-subtle pt-3">
    <div class="flex flex-wrap items-baseline justify-between gap-2">
      <div>
        <h3 class="m-0 text-base font-semibold text-fg">{{ title }}</h3>
        <p class="m-0 mt-0.5 text-xs text-fg-muted">
          Engine-side broker traffic is captured automatically and correlated to this scope.
        </p>
      </div>
      <div class="flex items-center gap-1.5">
        <span class="text-xs text-fg-muted" aria-live="polite">
          {{ liveUpdates ? "Live updates" : "Updates paused" }}
        </span>
        <button class="btn btn-sm" @click="toggleLiveUpdates">
          {{ liveUpdates ? "Pause" : "Resume" }}
        </button>
        <button class="btn btn-sm" :disabled="loading" @click="refresh()">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
      </div>
    </div>

    <p v-if="error" class="error m-0 text-xs">{{ error }}</p>
    <p v-else-if="loading && !records.length" class="m-0 text-xs text-fg-muted">
      Loading broker messages…
    </p>
    <p v-else-if="!records.length" class="m-0 text-xs text-fg-muted">
      No broker messages have reached the engine for this scope yet.
    </p>
    <div v-else ref="messageViewport" class="broker-message-scroll grid gap-1.5">
      <details
        v-for="record in records"
        :key="String(record.id)"
        :data-message-id="String(record.id)"
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
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
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
const liveUpdates = ref(true);
const messageViewport = ref<HTMLElement | null>(null);
let pollTimer = 0;

interface MessageScrollAnchor {
  atTop: boolean;
  id: string | null;
  offset: number;
  fallbackTop: number;
}

function captureScrollAnchor(): MessageScrollAnchor | null {
  const viewport = messageViewport.value;

  if (!viewport) {
    return null;
  }

  const atTop = viewport.scrollTop <= 2;
  const viewportTop = viewport.getBoundingClientRect().top;
  const firstVisible = Array.from(viewport.querySelectorAll<HTMLElement>("[data-message-id]")).find(
    (element) => element.getBoundingClientRect().bottom > viewportTop,
  );

  return {
    atTop,
    id: firstVisible?.dataset.messageId ?? null,
    offset: firstVisible ? firstVisible.getBoundingClientRect().top - viewportTop : 0,
    fallbackTop: viewport.scrollTop,
  };
}

function restoreScrollAnchor(anchor: MessageScrollAnchor | null, resetScroll: boolean): void {
  const viewport = messageViewport.value;

  if (!viewport) {
    return;
  }

  if (resetScroll || anchor?.atTop) {
    viewport.scrollTop = 0;
    return;
  }

  const anchoredRow = Array.from(viewport.querySelectorAll<HTMLElement>("[data-message-id]")).find(
    (element) => element.dataset.messageId === anchor?.id,
  );

  if (!anchor || !anchoredRow) {
    viewport.scrollTop = anchor?.fallbackTop ?? 0;
    return;
  }

  const currentOffset =
    anchoredRow.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
  viewport.scrollTop += currentOffset - anchor.offset;
}

async function refresh(resetScroll = false): Promise<void> {
  if (loading.value) {
    return;
  }

  loading.value = true;
  error.value = "";

  try {
    const nextRecords = await listBrokerMessages({
      workflowRunId: props.workflowRunId ?? undefined,
      pipelineRunId: props.pipelineRunId ?? undefined,
      limit: 250,
    });
    const anchor = resetScroll ? null : captureScrollAnchor();
    records.value = nextRecords;
    await nextTick();
    restoreScrollAnchor(anchor, resetScroll);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

function toggleLiveUpdates(): void {
  liveUpdates.value = !liveUpdates.value;

  if (liveUpdates.value) {
    void refresh();
  }
}

watch(
  () => [props.workflowRunId, props.pipelineRunId],
  () => void refresh(true),
  { immediate: true },
);

onMounted(() => {
  pollTimer = window.setInterval(() => {
    if (liveUpdates.value) {
      void refresh();
    }
  }, 1500);
});

onBeforeUnmount(() => {
  window.clearInterval(pollTimer);
});
</script>

<style scoped>
.broker-message-scroll {
  max-height: min(52dvh, 520px);
  overflow-y: scroll;
  overscroll-behavior: contain;
  padding-right: 4px;
  scrollbar-gutter: stable;
}
</style>
