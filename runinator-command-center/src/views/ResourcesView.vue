<template>
  <section class="pane resources-pane">
    <SplitPane class="split" :storage-key="`command-center.resources.${endpoint}.split`" :initial-first-pct="58" :min-first="420" :min-second="340" collapsible-second>
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
              <button class="btn btn-primary" :disabled="!resourcesStore.canResolveApproval" @click="resourcesStore.resolveApproval('approve')">
                <Icon name="approve" />
                <span>Approve</span>
              </button>
              <button class="btn btn-danger" :disabled="!resourcesStore.canResolveApproval" @click="resourcesStore.resolveApproval('reject')">
                <Icon name="reject" />
                <span>Reject</span>
              </button>
            </template>
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
                <th>External ID</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="record in resourcesStore.filteredResourceRecords"
                :key="String(record.id ?? JSON.stringify(record))"
                :class="{ selected: resourcesStore.selectedResourceRecord === record, danger: isBadStatus(record.status), success: isGoodStatus(record.status) }"
                @click="resourcesStore.selectedResourceRecord = record"
              >
                <td>{{ record.id ?? "" }}</td>
                <td>{{ record.provider ?? "" }}</td>
                <td>{{ resourcesStore.recordType(record) }}</td>
                <td><StatusBadge :status="record.status" /></td>
                <td>{{ resourcesStore.recordSummary(record) }}</td>
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
        <pre class="output">{{ resourcesStore.selectedResourceRecord ? pretty(resourcesStore.selectedResourceRecord) : "" }}</pre>
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
import { pretty } from "../utils/format";
import { isBadStatus, isGoodStatus } from "../utils/status";

const props = withDefaults(
  defineProps<{ endpoint?: string; title?: string }>(),
  { endpoint: "external_items", title: "" }
);

const resourcesStore = useResourcesStore();

const title = computed(() => props.title || labelFor(props.endpoint));

function labelFor(endpoint: string): string {
  return resourcesStore.resources.find((resource) => resource.endpoint === endpoint)?.label ?? "Records";
}

async function refresh() {
  await resourcesStore.refreshResourcesFor(props.endpoint);
}

onMounted(refresh);
watch(() => props.endpoint, refresh);
</script>
