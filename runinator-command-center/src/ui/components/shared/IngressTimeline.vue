<template>
  <section class="grid gap-2 border-t border-border-subtle pt-3">
    <div class="flex items-baseline justify-between gap-2">
      <h2 class="m-0 text-base font-semibold text-fg">Ingress timeline</h2>
      <span class="text-xs text-fg-muted">Admission scope and correlation key</span>
    </div>
    <form class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2" @submit.prevent="load">
      <input
        v-model.trim="scope"
        class="input"
        required
        data-validation="identifier"
        placeholder="scope"
        aria-label="Ingress scope"
      />
      <input
        v-model.trim="correlationKey"
        class="input"
        required
        placeholder="correlation key"
        aria-label="Ingress correlation key"
      />
      <button class="btn" type="submit" :disabled="busy || !scope || !correlationKey">
        Inspect
      </button>
    </form>
    <p v-if="error" class="m-0 text-sm text-danger">{{ error }}</p>
    <div v-if="admission" class="rounded border border-border-subtle bg-surface-subtle p-2 text-sm">
      <div class="flex flex-wrap gap-x-4 gap-y-1">
        <span><strong>Generation</strong> {{ admission.generation }}</span>
        <span><strong>Status</strong> {{ admission.status }}</span>
        <span><strong>Target</strong> {{ admission.target.kind }} · {{ admission.target.id }}</span>
        <span
          ><strong>Bound run</strong>
          {{ admission.workflow_run_id ?? admission.pipeline_run_id ?? "pending" }}</span
        >
        <span><strong>Queued</strong> {{ queuedCount }}</span>
      </div>
    </div>
    <ol v-if="events.length" class="m-0 grid list-none gap-1 p-0">
      <li
        v-for="event in events"
        :key="event.id"
        class="grid grid-cols-[4rem_1fr_auto] gap-2 rounded border border-border-subtle px-2 py-1 text-xs"
      >
        <span class="font-mono text-fg-muted">#{{ event.sequence }}</span>
        <span
          ><strong>{{ event.event_type }}</strong> · {{ event.source }} · {{ event.event_id }}</span
        >
        <span
          >{{ event.disposition
          }}<template v-if="event.queue_state !== 'none'">
            · {{ event.queue_state }}</template
          ></span
        >
      </li>
    </ol>
    <p v-else-if="admission && !busy" class="m-0 text-sm text-fg-muted">
      No ingress events recorded.
    </p>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef } from "vue";
import { fetchIngressAdmission, fetchIngressTimeline } from "../../../core/api/commandCenterApi";
import type { IngressAdmission, IngressInboxEntry } from "../../../core/domain/models";

const scope = ref("");
const correlationKey = ref("");
const admission = shallowRef<IngressAdmission | null>(null);
const events = shallowRef<IngressInboxEntry[]>([]);
const busy = ref(false);
const error = ref("");
const queuedCount = computed(
  () =>
    events.value.filter(
      (event) => event.queue_state === "queued" || event.queue_state === "claimed",
    ).length,
);

async function load() {
  busy.value = true;
  error.value = "";

  try {
    [admission.value, events.value] = await Promise.all([
      fetchIngressAdmission(scope.value, correlationKey.value),
      fetchIngressTimeline(scope.value, correlationKey.value),
    ]);
  } catch (reason) {
    admission.value = null;
    events.value = [];
    error.value = String(reason instanceof Error ? reason.message : reason);
  } finally {
    busy.value = false;
  }
}
</script>
