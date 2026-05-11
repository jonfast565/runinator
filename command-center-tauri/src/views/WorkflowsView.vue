<template>
  <section class="pane workflows-pane">
    <SplitPane class="workflow-layout" storage-key="command-center.workflows.list-split" :initial-first-pct="20" :min-first="240" :min-second="720">
      <template #first>
      <div class="panel workflow-list">
        <div class="panel-toolbar">
          <h2>Workflows</h2>
          <button @click="workflows.addWorkflow">New</button>
        </div>
        <DataTable>
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Version</th>
                <th>State</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="workflow in workflows.filteredWorkflows"
                :key="workflow.id ?? workflow.name"
                :class="{ selected: workflows.selectedWorkflowId === workflow.id, muted: !workflow.enabled }"
                @click="workflows.selectWorkflow(workflow)"
              >
                <td>{{ workflow.name }}</td>
                <td>{{ workflow.version }}</td>
                <td><StatusBadge :status="workflow.enabled" /></td>
              </tr>
            </tbody>
          </table>
        </DataTable>
      </div>
      </template>

      <template #second>
        <SplitPane class="workflow-main-split" storage-key="command-center.workflows.inspector-split" :initial-first-pct="64" :min-first="360" :min-second="320">
          <template #first>
            <WorkflowCanvas />
          </template>
          <template #second>
            <WorkflowInspector />
          </template>
        </SplitPane>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import WorkflowCanvas from "../components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "../components/workflow/WorkflowInspector.vue";
import DataTable from "../components/shared/DataTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../stores/workflows";

const workflows = useWorkflowsStore();
</script>
