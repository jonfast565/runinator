<template>
  <section class="pane workflows-pane">
    <SplitPane class="workflow-layout" storage-key="command-center.workflows.list-split" :initial-first-pct="20" :min-first="240" :min-second="720">
      <template #first>
        <div class="panel workflow-list">
          <div class="panel-toolbar">
            <div class="workflow-list-copy">
              <h2>Workflows</h2>
              <p>Browse definitions, select one to edit, or create a new workflow.</p>
            </div>
            <button class="btn btn-primary" @click="workflows.addWorkflow">
              <Icon name="plus" />
              <span>New</span>
            </button>
          </div>
          <div class="workflow-list-summary">
            <div>
              <span>Visible</span>
              <strong>{{ workflows.filteredWorkflows.length }}</strong>
            </div>
            <div>
              <span>Disabled</span>
              <strong>{{ disabledWorkflowCount }}</strong>
            </div>
            <div>
              <span>Selected</span>
              <strong>{{ selectedWorkflowLabel }}</strong>
            </div>
          </div>
          <div v-if="!workflows.filteredWorkflows.length" class="workflow-empty">No workflows match the current view.</div>
          <DataTable v-else>
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
    <WorkflowStepEditorModal v-if="workflows.stepEditorOpen" />
    <WorkflowRunInputModal v-if="workflows.runInputOpen" />
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import WorkflowCanvas from "../components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "../components/workflow/WorkflowInspector.vue";
import WorkflowStepEditorModal from "../components/workflow/WorkflowStepEditorModal.vue";
import WorkflowRunInputModal from "../components/workflow/WorkflowRunInputModal.vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../stores/workflows";

const workflows = useWorkflowsStore();
const disabledWorkflowCount = computed(() => workflows.filteredWorkflows.filter((workflow) => !workflow.enabled).length);
const selectedWorkflowLabel = computed(() => workflows.selectedWorkflow?.name ?? "None");
</script>

<style scoped>
.workflows-pane {
  overflow: hidden;
}

.workflow-list {
  min-height: 0;
}

.workflow-list-copy {
  display: grid;
  gap: 4px;
}

.workflow-list-copy p {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.workflow-list-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.workflow-list-summary div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.workflow-list-summary span {
  color: var(--text-muted);
  font-size: 12px;
}

.workflow-list-summary strong {
  color: var(--text);
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workflow-empty {
  color: var(--text-muted);
  padding: 6px 2px;
}

@media (max-width: 980px) {
  .workflow-list-summary {
    grid-template-columns: 1fr;
  }
}
</style>
