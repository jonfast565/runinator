<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      :storage-key="`command-center.resources.${endpoint}.split`"
      :initial-first-pct="58"
      :min-first="420"
      :min-second="340"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="!!resourcesStore.selectedResourceRecord"
    >
      <template #first>
        <div class="panel">
          <PanelHeader
            :title="title"
            :description="paneDescription"
            :icon="endpoint === 'approvals' ? 'approve' : 'tag'"
            :eyebrow="endpoint === 'approvals' ? 'Human decisions' : 'Provider records'"
          >
            <div class="btn-row">
              <button class="btn" :disabled="loadingResources" @click="refresh">
                <LoadingSpinner v-if="loadingResources" size="sm" label="Refreshing resources" />
                <Icon v-else name="refresh" />
                <span>Refresh</span>
              </button>
              <template v-if="endpoint === 'approvals'">
                <label class="inline-flex items-center gap-1.5 text-xs text-fg-muted">
                  <input v-model="resourcesStore.hideResolved" type="checkbox" class="w-auto" />
                  <span>Hide resolved</span>
                </label>
                <button
                  class="btn btn-primary"
                  :disabled="!resourcesStore.canResolveApproval"
                  @click="resolveApproval('approve')"
                >
                  <Icon name="approve" />
                  <span>Approve</span>
                </button>
                <button
                  class="btn btn-danger"
                  :disabled="!resourcesStore.canResolveApproval"
                  @click="resolveApproval('reject')"
                >
                  <Icon name="reject" />
                  <span>Reject</span>
                </button>
              </template>
              <button
                v-if="endpoint === 'automation_events'"
                class="btn"
                :disabled="!resourcesStore.canDeleteSelected"
                @click="deleteSelected"
              >
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
          </PanelHeader>
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard
              label="Visible records"
              :value="resourcesStore.filteredResourceRecords.length"
            />
            <MetricCard
              :label="endpoint === 'approvals' ? 'Awaiting review' : 'Providers'"
              :value="endpoint === 'approvals' ? pendingRecordCount : providerCount"
            />
            <MetricCard
              label="Selected"
              :value="resourcesStore.selectedResourceRecord ? 'Ready' : '—'"
            />
          </div>
          <DataTable>
            <thead>
              <tr>
                <th>ID</th>
                <th class="col-low">Provider</th>
                <th class="col-low">Type</th>
                <th>Status</th>
                <th>Summary</th>
                <th v-if="endpoint === 'approvals'" class="col-low">Resolved by</th>
                <th class="col-low">External ID</th>
              </tr>
            </thead>
            <tbody>
              <tr v-if="loadingResources && !resourcesStore.resourceRecords.length">
                <td
                  :colspan="endpoint === 'approvals' ? 7 : 6"
                  class="px-3.5 py-3.5 text-center text-fg-muted"
                >
                  <LoadingPanel
                    compact
                    :message="loadingResourcesMessage || `Loading ${title.toLowerCase()}…`"
                  />
                </td>
              </tr>
              <tr v-else-if="!resourcesStore.filteredResourceRecords.length">
                <td :colspan="endpoint === 'approvals' ? 7 : 6" class="!p-0 hover:!bg-transparent">
                  <EmptyState
                    compact
                    :icon="resourcesStore.resourceRecords.length ? 'search' : 'box'"
                    :title="
                      resourcesStore.resourceRecords.length
                        ? 'No matches'
                        : `No ${title.toLowerCase()} yet`
                    "
                    :description="
                      resourcesStore.resourceRecords.length
                        ? `No records match “${app.searchQuery}”.`
                        : `${title} raised by providers and workflow runs appear here.`
                    "
                  />
                </td>
              </tr>
              <tr
                v-for="record in resourcesStore.filteredResourceRecords"
                :key="String(record.id ?? JSON.stringify(record))"
                class="cursor-pointer"
                :class="{
                  selected: resourcesStore.selectedResourceRecord === record,
                  danger: isBadStatus(record.status),
                  success: isGoodStatus(record.status),
                  'opacity-55': endpoint === 'approvals' && resourcesStore.isResolved(record),
                }"
                @click="resourcesStore.selectedResourceRecord = record"
              >
                <td>{{ record.id ?? "" }}</td>
                <td class="col-low">{{ record.provider ?? "" }}</td>
                <td class="col-low">{{ resourcesStore.recordType(record) }}</td>
                <td><StatusBadge :status="record.status as string" /></td>
                <td>{{ resourcesStore.recordSummary(record) }}</td>
                <td v-if="endpoint === 'approvals'" class="col-low whitespace-nowrap">
                  <template v-if="resourcesStore.isResolved(record)">
                    {{ record.resolved_by ?? "—" }}
                    <span v-if="record.resolved_at" class="block text-[11px] text-fg-muted">{{
                      formatDate(record.resolved_at as string | null | undefined)
                    }}</span>
                  </template>
                </td>
                <td class="col-low">
                  {{ record.external_id ?? record.key ?? record.url ?? "" }}
                </td>
              </tr>
            </tbody>
          </DataTable>
        </div>
      </template>
      <template #second>
        <div class="panel details overflow-hidden">
          <MobileBackBar @back="resourcesStore.selectedResourceRecord = null" />
          <h2 class="m-0 text-base font-semibold text-fg">Record Detail</h2>
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
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useResourcesStore } from "../../ui/adapters/pinia/resources";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOperationLoading } from "../composables/useOperationLoading";
import { formatDate, pretty } from "../../core/utils/format";
import { isBadStatus, isGoodStatus } from "../../core/utils/status";

const props = withDefaults(defineProps<{ endpoint?: string; title?: string }>(), {
  endpoint: "external_items",
  title: "",
});

const resourcesStore = useResourcesStore();
const orgs = useOrgsStore();
const app = useAppStore();
const { isLoading: loadingResources, loadingMessage: loadingResourcesMessage } =
  useOperationLoading(["Refreshing resources", "Loading workflow approvals"]);

const title = computed(() => props.title || labelFor(props.endpoint));
const paneDescription = computed(() =>
  props.endpoint === "approvals"
    ? "Select a request and review its full detail before approving or rejecting it."
    : props.endpoint === "automation_events"
      ? "Select a record to inspect the complete payload before deleting it."
      : "Select a record to inspect its provider-owned fields and workflow context.",
);
const pendingRecordCount = computed(
  () =>
    resourcesStore.filteredResourceRecords.filter((record) => !resourcesStore.isResolved(record))
      .length,
);
const providerCount = computed(
  () =>
    new Set(
      resourcesStore.filteredResourceRecords
        .map((record) => (typeof record.provider === "string" ? record.provider : ""))
        .filter(Boolean),
    ).size,
);

function labelFor(endpoint: string): string {
  return (
    resourcesStore.resources.find((resource) => resource.endpoint === endpoint)?.label ?? "Records"
  );
}

async function refresh() {
  resourcesStore.clearResources();
  await resourcesStore.refreshResourcesFor(props.endpoint);
}

async function resolveApproval(action: "approve" | "reject") {
  const record = resourcesStore.selectedResourceRecord;

  if (!record) {
    return;
  }

  const recordSummary = resourcesStore.recordSummary(record);
  const summary = recordSummary.trim() ? recordSummary : "selected request";
  const verb = action === "approve" ? "Approve" : "Reject";

  if (!window.confirm(`${verb} “${summary}”? Review the record detail before continuing.`)) {
    return;
  }

  await resourcesStore.resolveApproval(action);
}

async function deleteSelected() {
  const record = resourcesStore.selectedResourceRecord;

  if (!record || !window.confirm("Delete the selected automation event? This cannot be undone.")) {
    return;
  }

  await resourcesStore.deleteSelected();
}

onMounted(refresh);
watch(() => props.endpoint, refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
