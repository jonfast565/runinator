<template>
  <section class="pane flex h-full min-h-0 flex-col gap-3 p-4">
    <header class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-semibold text-fg">Orchestrations</h1>
        <p class="text-sm text-fg-muted">Correlation generations, immutable execution epochs, and control evidence.</p>
      </div>
      <div class="flex gap-2">
        <select v-model="status" class="input" @change="refresh">
          <option value="">All statuses</option>
          <option v-for="item in statuses" :key="item" :value="item">{{ item }}</option>
        </select>
        <button class="button" :disabled="store.loading" @click="refresh">Refresh</button>
      </div>
    </header>

    <p v-if="store.error" class="rounded border border-danger/40 bg-danger/10 p-3 text-sm text-danger">{{ store.error }}</p>
    <div class="grid min-h-0 flex-1 gap-3 lg:grid-cols-[22rem_minmax(0,1fr)]">
      <aside class="min-h-0 overflow-auto rounded border border-border bg-surface">
        <button
          v-for="binding in store.bindings"
          :key="binding.id"
          class="block w-full border-b border-border p-3 text-left hover:bg-surface-raised"
          :class="{ 'bg-surface-raised': binding.id === store.selectedId }"
          @click="store.select(binding.id)"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="truncate font-medium text-fg">{{ binding.correlation_key }}</span>
            <span class="rounded bg-surface-raised px-2 py-0.5 text-xs text-fg-muted">{{ binding.status }}</span>
          </div>
          <div class="mt-1 truncate text-xs text-fg-muted">{{ binding.scope }} · generation {{ binding.generation }}</div>
        </button>
        <p v-if="!store.loading && store.bindings.length === 0" class="p-4 text-sm text-fg-muted">No orchestrations match these filters.</p>
      </aside>

      <main v-if="store.selected" class="min-h-0 overflow-auto rounded border border-border bg-surface p-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 class="text-lg font-semibold text-fg">{{ store.selected.correlation_key }}</h2>
            <div class="mt-2 flex flex-wrap gap-2 text-xs text-fg-muted">
              <span class="rounded bg-surface-raised px-2 py-1">generation {{ store.selected.generation }}</span>
              <span class="rounded bg-surface-raised px-2 py-1">epoch {{ store.selected.current_epoch }}</span>
              <span class="rounded bg-surface-raised px-2 py-1">phase {{ store.selected.current_phase || "—" }}</span>
              <span class="rounded bg-surface-raised px-2 py-1">CAS {{ store.selected.version }}</span>
              <span class="rounded bg-surface-raised px-2 py-1">revision {{ store.selected.pipeline_revision }}</span>
            </div>
          </div>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="(_, name) in store.selected.policy.intents"
              :key="name"
              class="button"
              @click="openIntent(String(name))"
            >{{ name }}</button>
          </div>
        </div>

        <nav class="mt-5 flex flex-wrap gap-1 border-b border-border">
          <button v-for="tab in tabs" :key="tab" class="px-3 py-2 text-sm" :class="tab === activeTab ? 'border-b-2 border-accent text-fg' : 'text-fg-muted'" @click="activeTab = tab">{{ tab }}</button>
        </nav>
        <div class="mt-4">
          <div v-if="activeTab === 'Timeline'" class="space-y-2">
            <article v-for="event in store.events" :key="event.id" class="rounded border border-border p-3 text-sm">
              <div class="flex justify-between gap-3"><strong>#{{ event.sequence }} {{ event.winner || "observed" }}</strong><span class="text-fg-muted">{{ event.disposition }}</span></div>
              <p class="mt-1 text-xs text-fg-muted">matched: {{ event.matched_intents.join(", ") || "none" }} · suppressed: {{ event.suppressed_intents.join(", ") || "none" }}</p>
            </article>
          </div>
          <pre v-else-if="activeTab === 'Epochs'" class="overflow-auto text-xs">{{ pretty(store.epochs) }}</pre>
          <pre v-else-if="activeTab === 'Evidence'" class="overflow-auto text-xs">{{ pretty(store.evidence) }}</pre>
          <pre v-else-if="activeTab === 'Commands'" class="overflow-auto text-xs">{{ pretty(store.commands) }}</pre>
          <pre v-else class="overflow-auto text-xs">{{ pretty(store.selected) }}</pre>
        </div>
      </main>
      <main v-else class="rounded border border-border bg-surface p-6 text-sm text-fg-muted">Select an orchestration.</main>
    </div>

    <div v-if="intentName" class="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" @click.self="intentName = null">
      <form class="w-full max-w-md rounded border border-border bg-surface p-4 shadow-xl" @submit.prevent="submitIntent">
        <h2 class="font-semibold text-fg">Dispatch {{ intentName }}</h2>
        <label class="mt-3 block text-sm text-fg-muted">Reason</label>
        <textarea v-model="reason" required class="input mt-1 min-h-24 w-full" />
        <div class="mt-4 flex justify-end gap-2"><button type="button" class="button" @click="intentName = null">Cancel</button><button class="button" type="submit">Dispatch</button></div>
      </form>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useOrchestrationsStore } from "../adapters/pinia/orchestrations";

const store = useOrchestrationsStore();
const statuses = ["pending", "running", "waiting", "suspended", "completed", "failed", "terminated"];
const tabs = ["Timeline", "Epochs", "Evidence", "Commands", "Raw"];
const status = ref("");
const activeTab = ref("Timeline");
const intentName = ref<string | null>(null);
const reason = ref("");

function refresh(): void {
  void store.refresh(status.value ? { status: status.value } : {});
}

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function openIntent(name: string): void {
  intentName.value = name;
  reason.value = "";
}

async function submitIntent(): Promise<void> {
  if (!intentName.value || !reason.value.trim()) {
    return;
  }

  await store.dispatch(intentName.value, reason.value.trim());
  intentName.value = null;
}

onMounted(refresh);
</script>
