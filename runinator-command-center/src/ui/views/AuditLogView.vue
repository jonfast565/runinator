<template>
  <section class="pane">
    <div class="panel flex min-h-0 flex-col">
      <div class="panel-toolbar">
        <h2 class="m-0 text-base font-semibold text-fg">Audit Log</h2>
        <div class="flex items-center gap-2">
          <input
            v-model="action"
            class="input"
            placeholder="Filter by action (e.g. auth.login)"
            @keyup.enter="refresh"
          />
          <Button variant="default" :loading="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </Button>
        </div>
      </div>

      <DataTable
        :columns="columns"
        :rows="rows"
        row-key="id"
        :page-size="50"
        :loading="loading"
        loading-message="Loading audit log…"
        responsive="cards"
        initial-sort-key="created_at"
        initial-sort-dir="desc"
        empty-icon="list"
        empty-title="No audit entries"
        empty-description="Logins and authorization denials are recorded here."
      >
        <template #cell-created_at="{ row }">{{ formatDate(row.created_at as string) }}</template>
        <template #cell-action="{ row }"
          ><span class="font-mono">{{ row.action }}</span></template
        >
        <template #cell-outcome="{ row }">
          <span
            class="rounded-pill px-2 py-0.5 text-xs capitalize"
            :class="
              row.outcome === 'success'
                ? 'bg-success-bg text-success-fg'
                : 'bg-danger-bg text-danger-fg'
            "
            >{{ row.outcome }}</span
          >
        </template>
        <template #cell-actor="{ row }"
          ><span class="font-mono">{{ row.actor_id || row.actor_kind || "-" }}</span></template
        >
        <template #cell-resource="{ row }"
          ><span class="font-mono">{{ resourceLabel(row) }}</span></template
        >
        <template #cell-detail="{ row }"
          ><span class="max-w-[420px]">{{ row.detail || "-" }}</span></template
        >
      </DataTable>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import Button from "../components/shared/Button.vue";
import DataTable, { type DataTableColumn } from "../components/shared/DataTable.vue";
import { auditLogService } from "../../core/services";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import type { JsonRecord } from "../../core/domain/models";
import { displayValue } from "../../core/utils/values";
import { formatDate } from "../../core/utils/format";

const app = useAppStore();
const orgs = useOrgsStore();
const loading = ref(false);
const rows = ref<JsonRecord[]>([]);
const action = ref("");

const columns: DataTableColumn<JsonRecord>[] = [
  { key: "created_at", label: "Time", sortable: true },
  { key: "action", label: "Action", sortable: true },
  { key: "outcome", label: "Outcome", sortable: true },
  {
    key: "actor",
    label: "Actor",
    sortable: true,
    value: (row: JsonRecord) => displayValue(row.actor_id ?? row.actor_kind ?? "-"),
  },
  { key: "resource", label: "Resource", value: (row: JsonRecord) => resourceLabel(row) },
  { key: "detail", label: "Detail" },
];

function resourceLabel(row: JsonRecord): string {
  if (!row.resource_type) {
    return "-";
  }

  return `${displayValue(row.resource_type)}:${displayValue(row.resource_id ?? "?")}`;
}

async function refresh() {
  loading.value = true;
  rows.value = [];

  try {
    await app.runOperation("Loading audit log", async () => {
      rows.value = await auditLogService.list(action.value || undefined, 200);
    });
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
