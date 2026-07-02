<template>
  <section class="pane resources-pane">
    <SplitPane
      class="split"
      :storage-key="`command-center.resources.${endpoint}.split`"
      :initial-first-pct="58"
      :min-first="420"
      :min-second="340"
      collapsible-second
    >
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <h2>{{ title }}</h2>
            <div class="btn-row">
              <button class="btn" @click="refresh">
                <Icon name="refresh" />
                <span>Refresh</span>
              </button>
              <template v-if="endpoint === 'approvals'">
                <label class="filter-toggle">
                  <input v-model="resourcesStore.hideResolved" type="checkbox" />
                  <span>Hide resolved</span>
                </label>
                <button
                  class="btn btn-primary"
                  :disabled="!resourcesStore.canResolveApproval"
                  @click="resourcesStore.resolveApproval('approve')"
                >
                  <Icon name="approve" />
                  <span>Approve</span>
                </button>
                <button
                  class="btn btn-danger"
                  :disabled="!resourcesStore.canResolveApproval"
                  @click="resourcesStore.resolveApproval('reject')"
                >
                  <Icon name="reject" />
                  <span>Reject</span>
                </button>
              </template>
              <button
                v-if="endpoint === 'automation_events'"
                class="btn"
                :disabled="!resourcesStore.canDeleteSelected"
                @click="resourcesStore.deleteSelected()"
              >
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
          </div>
          <DataTable>
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Provider</th>
                  <th>Type</th>
                  <th>Status</th>
                  <th>Summary</th>
                  <th v-if="endpoint === 'approvals'">Resolved by</th>
                  <th>External ID</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="!resourcesStore.filteredResourceRecords.length">
                  <td :colspan="endpoint === 'approvals' ? 7 : 6" class="empty-cell">
                    {{
                      resourcesStore.resourceRecords.length
                        ? `No records match “${app.searchQuery}”.`
                        : `No ${title.toLowerCase()} yet.`
                    }}
                  </td>
                </tr>
                <tr
                  v-for="record in resourcesStore.filteredResourceRecords"
                  :key="String(record.id ?? JSON.stringify(record))"
                  :class="{
                    selected: resourcesStore.selectedResourceRecord === record,
                    danger: isBadStatus(record.status),
                    success: isGoodStatus(record.status),
                    resolved: endpoint === 'approvals' && resourcesStore.isResolved(record),
                  }"
                  @click="resourcesStore.selectedResourceRecord = record"
                >
                  <td>{{ record.id ?? "" }}</td>
                  <td>{{ record.provider ?? "" }}</td>
                  <td>{{ resourcesStore.recordType(record) }}</td>
                  <td><StatusBadge :status="record.status as string" /></td>
                  <td>{{ resourcesStore.recordSummary(record) }}</td>
                  <td v-if="endpoint === 'approvals'" class="resolver-cell">
                    <template v-if="resourcesStore.isResolved(record)">
                      {{ record.resolved_by ?? "—" }}
                      <span v-if="record.resolved_at" class="resolver-time">{{
                        formatDate(record.resolved_at as string | null | undefined)
                      }}</span>
                    </template>
                  </td>
                  <td>{{ record.external_id ?? record.key ?? record.url ?? "" }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
      <template #second>
        <div class="panel details">
          <h2>Record Detail</h2>
          <pre class="output">{{
            resourcesStore.selectedResourceRecord
              ? pretty(resourcesStore.selectedResourceRecord)
              : ""
          }}</pre>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useResourcesStore } from "../stores/resources";
import { useOrgsStore } from "../stores/orgs";
import { useAppStore } from "../stores/app";
import { formatDate, pretty } from "../utils/format";
import { isBadStatus, isGoodStatus } from "../utils/status";

const props = withDefaults(defineProps<{ endpoint?: string; title?: string }>(), {
  endpoint: "external_items",
  title: "",
});

const resourcesStore = useResourcesStore();
const orgs = useOrgsStore();
const app = useAppStore();

const title = computed(() => props.title || labelFor(props.endpoint));

function labelFor(endpoint: string): string {
  return (
    resourcesStore.resources.find((resource) => resource.endpoint === endpoint)?.label ?? "Records"
  );
}

async function refresh() {
  resourcesStore.clearResources();
  await resourcesStore.refreshResourcesFor(props.endpoint);
}

onMounted(refresh);
watch(() => props.endpoint, refresh);
watch(() => orgs.activeOrgId, refresh);
</script>

<style scoped>
.filter-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
  font-size: 12px;
}

.filter-toggle input {
  width: auto;
}

tr.resolved td {
  opacity: 0.55;
}

.empty-cell {
  color: var(--text-muted);
  text-align: center;
  padding: 14px;
}

.resolver-cell {
  white-space: nowrap;
}

.resolver-time {
  display: block;
  color: var(--text-muted);
  font-size: 11px;
}
</style>
