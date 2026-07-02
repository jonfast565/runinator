<template>
  <section class="pane resources-pane">
    <SplitPane
      class="split"
      storage-key="command-center.events.split"
      :initial-first-pct="62"
      :min-first="460"
      :min-second="320"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="!!resourcesStore.selectedResourceRecord"
    >
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <h2>Events</h2>
            <div class="btn-row">
              <button class="btn" @click="refresh">
                <Icon name="refresh" />
                <span>Refresh</span>
              </button>
            </div>
          </div>
          <DataTable>
            <table>
              <thead>
                <tr>
                  <th class="col-low">ID</th>
                  <th>Event Type</th>
                  <th>Message</th>
                  <th class="col-low">Provider</th>
                  <th class="col-low">Workflow Run</th>
                  <th class="col-low">Node</th>
                  <th>Created At</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="record in resourcesStore.filteredResourceRecords"
                  :key="String(record.id ?? JSON.stringify(record))"
                  :class="{ selected: resourcesStore.selectedResourceRecord === record }"
                  @click="resourcesStore.selectedResourceRecord = record"
                >
                  <td class="col-low">{{ record.id ?? "" }}</td>
                  <td>{{ eventType(record) }}</td>
                  <td>{{ eventMessage(record) }}</td>
                  <td class="col-low">{{ String(record.provider ?? "") }}</td>
                  <td class="col-low">{{ String(record.workflow_run_id ?? "") }}</td>
                  <td class="col-low">
                    {{ String(record.node_id ?? record.workflow_node_run_id ?? "") }}
                  </td>
                  <td>{{ String(record.created_at ?? "") }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
      <template #second>
        <div class="panel details">
          <MobileBackBar @back="resourcesStore.selectedResourceRecord = null" />
          <h2>Event Detail</h2>
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
import { onMounted, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useResourcesStore } from "../../ui/adapters/pinia/resources";
import type { JsonRecord } from "../../core/domain/models";
import { pretty } from "../../core/utils/format";
import { displayValue } from "../../core/utils/values";

const resourcesStore = useResourcesStore();
const orgs = useOrgsStore();
const endpoint = "automation_events";

async function refresh() {
  resourcesStore.clearResources();
  await resourcesStore.refreshResourcesFor(endpoint);
}

function eventType(record: JsonRecord): string {
  return displayValue(record.event_type ?? record.resource_type ?? "");
}

function eventMessage(record: JsonRecord): string {
  return displayValue(record.message ?? record.title ?? record.summary ?? "");
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
