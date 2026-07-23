<template>
  <section class="pane">
    <div class="panel flex min-h-0 flex-col">
      <div class="panel-toolbar">
        <h2 class="m-0 text-base font-semibold text-fg">Dead Letters</h2>
        <div class="flex items-center gap-2">
          <select v-model="channel" class="input" @change="refresh">
            <option value="">All channels</option>
            <option value="result">result</option>
            <option value="ingress">ingress</option>
          </select>
          <Button variant="default" :loading="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </Button>
        </div>
      </div>

      <EmptyState
        v-if="loading && !rows.length"
        compact
        loading
        title="Loading dead letters"
        loading-message="Loading dead letters…"
      />
      <EmptyState
        v-else-if="!rows.length"
        compact
        icon="flag"
        title="No dead-lettered messages"
        description="Failed broker deliveries that exhaust their retries appear here."
      />

      <div v-else class="table-scroll min-h-0 flex-1">
        <table class="w-full border-collapse text-[13px]">
          <thead>
            <tr>
              <th>Time</th>
              <th class="col-low">Channel</th>
              <th class="col-low">Attempts</th>
              <th>Error</th>
              <th class="col-low">Event</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="row in rows" :key="String(row.id)">
              <tr class="cursor-pointer hover:bg-surface-subtle" @click="toggle(String(row.id))">
                <td class="border-b border-border px-2.5 py-2 align-top text-left">
                  {{ formatDate(row.created_at as string) }}
                </td>
                <td class="col-low border-b border-border px-2.5 py-2 align-top text-left">
                  <span class="rounded-pill bg-surface-subtle px-2 py-0.5 text-xs">{{
                    row.channel
                  }}</span>
                </td>
                <td class="col-low border-b border-border px-2.5 py-2 align-top text-left">
                  {{ row.attempts }}
                </td>
                <td
                  class="max-w-[420px] overflow-hidden text-ellipsis border-b border-border px-2.5 py-2 align-top text-left text-danger-fg"
                >
                  {{ row.error }}
                </td>
                <td
                  class="col-low border-b border-border px-2.5 py-2 align-top text-left font-mono"
                >
                  {{ row.event_id || row.dedupe_key || "-" }}
                </td>
              </tr>
              <tr v-if="expanded === String(row.id)">
                <td colspan="5" class="border-b border-border px-2.5 py-2">
                  <pre
                    class="m-0 overflow-auto rounded-md border border-border bg-surface-sunken p-3 font-mono text-xs leading-snug"
                    >{{ pretty(row.payload ?? {}) }}</pre
                  >
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import Button from "../components/shared/Button.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import { deadLettersService } from "../../core/services";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import type { JsonRecord } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";

const app = useAppStore();
const orgs = useOrgsStore();
const loading = ref(false);
const rows = ref<JsonRecord[]>([]);
const channel = ref("");
const expanded = ref<string | null>(null);

function toggle(id: string) {
  expanded.value = expanded.value === id ? null : id;
}

async function refresh() {
  loading.value = true;
  rows.value = [];
  expanded.value = null;

  try {
    await app.runOperation("Loading dead letters", async () => {
      rows.value = await deadLettersService.list(channel.value || undefined, 200);
    });
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
