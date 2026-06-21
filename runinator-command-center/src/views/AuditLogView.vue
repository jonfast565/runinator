<template>
  <section class="pane audit-pane">
    <div class="panel audit-panel">
      <div class="panel-toolbar">
        <h2>Audit Log</h2>
        <div class="audit-controls">
          <input
            v-model="action"
            class="input"
            placeholder="Filter by action (e.g. auth.login)"
            @keyup.enter="refresh"
          />
          <button class="btn" :disabled="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
      </div>

      <div v-if="!rows.length" class="empty-state">
        No audit entries. Logins and authorization denials are recorded here.
      </div>

      <div v-else class="audit-table-wrap">
        <table class="audit-table">
          <thead>
            <tr>
              <th>Time</th>
              <th>Action</th>
              <th>Outcome</th>
              <th>Actor</th>
              <th>Resource</th>
              <th>Detail</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="String(row.id)">
              <td>{{ formatDate(row.created_at as string) }}</td>
              <td class="mono">{{ row.action }}</td>
              <td>
                <span class="badge" :class="`badge-${row.outcome}`">{{ row.outcome }}</span>
              </td>
              <td class="mono">{{ row.actor_id || row.actor_kind || "-" }}</td>
              <td class="mono">{{ resourceLabel(row) }}</td>
              <td class="audit-detail">{{ row.detail || "-" }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import Icon from "../components/shared/Icon.vue";
import { listAuditLog } from "../api/commandCenterApi";
import { useAppStore } from "../stores/app";
import type { JsonRecord } from "../types/models";
import { formatDate } from "../utils/format";

const app = useAppStore();
const loading = ref(false);
const rows = ref<JsonRecord[]>([]);
const action = ref("");

function resourceLabel(row: JsonRecord): string {
  if (!row.resource_type) return "-";
  return `${row.resource_type}:${row.resource_id ?? "?"}`;
}

async function refresh() {
  loading.value = true;
  try {
    await app.runOperation("Loading audit log", async () => {
      rows.value = await listAuditLog(undefined, action.value || undefined, 200);
    });
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);
</script>

<style scoped>
.audit-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.audit-controls {
  display: flex;
  gap: 8px;
  align-items: center;
}

.audit-table-wrap {
  overflow: auto;
  min-height: 0;
}

.audit-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.audit-table th,
.audit-table td {
  text-align: left;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}

.badge {
  border-radius: var(--radius-pill);
  padding: 2px 8px;
  font-size: 12px;
  text-transform: capitalize;
}

.badge-success {
  background: var(--success-bg);
  color: var(--success-fg);
}

.badge-failure,
.badge-denied {
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.mono {
  font-family: var(--font-mono);
}

.audit-detail {
  max-width: 420px;
}
</style>
