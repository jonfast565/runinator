<template>
  <section class="pane workflows-pane">
    <div class="workflow-layout">
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

      <WorkflowCanvas />
      <WorkflowInspector />
    </div>
  </section>
</template>

<script setup lang="ts">
import WorkflowCanvas from "../components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "../components/workflow/WorkflowInspector.vue";
import DataTable from "../components/shared/DataTable.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../stores/workflows";

const workflows = useWorkflowsStore();
</script>
