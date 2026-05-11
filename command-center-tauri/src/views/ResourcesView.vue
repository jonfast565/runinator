<template>
  <section class="pane resources-pane">
    <SplitPane class="split" storage-key="command-center.resources.split" :initial-first-pct="58" :min-first="420" :min-second="340">
      <template #first>
      <div class="panel">
        <div class="panel-toolbar">
          <h2>Resources</h2>
          <div>
            <select v-model="resourcesStore.selectedResourceEndpoint" @change="resourcesStore.refreshResources">
              <option v-for="resource in resourcesStore.resources" :key="resource.endpoint" :value="resource.endpoint">
                {{ resource.label }}
              </option>
            </select>
            <button @click="resourcesStore.refreshResources">Refresh</button>
            <template v-if="resourcesStore.selectedResourceEndpoint === 'approvals'">
              <button :disabled="!resourcesStore.canResolveApproval" @click="resourcesStore.resolveApproval('approve')">Approve</button>
              <button :disabled="!resourcesStore.canResolveApproval" @click="resourcesStore.resolveApproval('reject')">Reject</button>
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
import DataTable from "../components/shared/DataTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useResourcesStore } from "../stores/resources";
import { pretty } from "../utils/format";
import { isBadStatus, isGoodStatus } from "../utils/status";

const resourcesStore = useResourcesStore();
</script>
