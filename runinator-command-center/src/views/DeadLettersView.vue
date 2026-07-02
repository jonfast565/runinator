<template>
  <section class="pane dlq-pane">
    <div class="panel dlq-panel">
      <div class="panel-toolbar">
        <h2>Dead Letters</h2>
        <div class="dlq-controls">
          <select v-model="channel" class="input" @change="refresh">
            <option value="">All channels</option>
            <option value="result">result</option>
            <option value="ingress">ingress</option>
          </select>
          <button class="btn" :disabled="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
      </div>

      <EmptyState
        v-if="!rows.length"
        compact
        icon="flag"
        title="No dead-lettered messages"
        description="Failed broker deliveries that exhaust their retries appear here."
      />

      <div v-else class="dlq-table-wrap">
        <table class="dlq-table">
          <thead>
            <tr>
              <th>Time</th>
              <th>Channel</th>
              <th>Attempts</th>
              <th>Error</th>
              <th>Event</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="row in rows" :key="String(row.id)">
              <tr class="dlq-row" @click="toggle(String(row.id))">
                <td>{{ formatDate(row.created_at as string) }}</td>
                <td>
                  <span class="badge">{{ row.channel }}</span>
                </td>
                <td>{{ row.attempts }}</td>
                <td class="dlq-error">{{ row.error }}</td>
                <td class="mono">{{ row.event_id || row.dedupe_key || "-" }}</td>
              </tr>
              <tr v-if="expanded === String(row.id)" class="dlq-detail-row">
                <td colspan="5">
                  <pre class="dlq-pre">{{ pretty(row.payload ?? {}) }}</pre>
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
import EmptyState from "../components/shared/EmptyState.vue";
import { listDeadLetters } from "../api/commandCenterApi";
import { useAppStore } from "../stores/app";
import { useOrgsStore } from "../stores/orgs";
import type { JsonRecord } from "../types/models";
import { formatDate, pretty } from "../utils/format";

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
      rows.value = await listDeadLetters(channel.value || undefined, 200);
    });
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>

<style scoped>
.dlq-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.dlq-controls {
  display: flex;
  gap: 8px;
  align-items: center;
}

.dlq-table-wrap {
  flex: 1;
  overflow: auto;
  min-height: 0;
}

.dlq-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.dlq-table th,
.dlq-table td {
  text-align: left;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}

.dlq-row {
  cursor: pointer;
}

.dlq-row:hover {
  background: var(--surface-subtle);
}

.dlq-error {
  color: var(--danger-fg);
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.badge {
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 2px 8px;
  font-size: 12px;
}

.mono {
  font-family: var(--font-mono);
}

.dlq-pre {
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
</style>
