<template>
  <section class="pane workflows-pane">
    <SplitPane class="workflow-layout" storage-key="command-center.workflows.list-split" :initial-first-pct="20" :min-first="240" :min-second="720" collapsible-first>
      <template #first>
        <div class="panel workflow-list">
          <div class="panel-toolbar">
            <div class="workflow-list-copy">
              <h2>Workflows</h2>
              <p>Browse definitions, select one to edit, or create a new workflow.</p>
            </div>
            <button class="btn btn-primary" @click="newWorkflow">
              <Icon name="plus" />
              <span>New</span>
            </button>
          </div>
          <div class="workflow-scope-filter">
            <label>Scope</label>
            <select v-model="scopeFilter">
              <option value="all">All</option>
              <option value="org">This org</option>
              <option value="global">Global</option>
            </select>
          </div>
          <div class="workflow-list-summary">
            <div>
              <span>Visible</span>
              <strong>{{ scopedWorkflows.length }}</strong>
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
          <EmptyState
            v-if="!workflows.workflows.length"
            compact
            icon="workflow"
            title="No workflows yet"
            description="Workflows orchestrate tasks as a state machine. Create one to start editing on the graph and WDL canvas."
          >
            <button class="btn btn-primary" @click="workflows.addWorkflow">
              <Icon name="plus" />
              <span>Create your first workflow</span>
            </button>
          </EmptyState>
          <EmptyState
            v-else-if="!scopedWorkflows.length"
            compact
            icon="search"
            title="No matches"
            :description="app.searchQuery ? `No workflows match “${app.searchQuery}”.` : 'No workflows match the current scope filter.'"
          />
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
                  v-for="workflow in scopedWorkflows"
                  :key="workflow.id ?? workflow.name"
                  :class="{ selected: workflows.selectedWorkflowId === workflow.id, muted: !workflow.enabled }"
                  @click="chooseWorkflow(workflow)"
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
        <SplitPane class="workflow-main-split" storage-key="command-center.workflows.inspector-split" :initial-first-pct="64" :min-first="360" :min-second="320" collapsible-second>
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
import { computed, ref } from "vue";
import WorkflowCanvas from "../components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "../components/workflow/WorkflowInspector.vue";
import WorkflowStepEditorModal from "../components/workflow/WorkflowStepEditorModal.vue";
import WorkflowRunInputModal from "../components/workflow/WorkflowRunInputModal.vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../stores/workflows";
import { useOrgsStore } from "../stores/orgs";
import { useAppStore } from "../stores/app";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const scopeFilter = ref<"all" | "org" | "global">("all");

// client-side scope filter on top of the server's already org-scoped list: "org" keeps only
// workflows owned by the active org, "global" keeps only unassigned (platform-global) ones.
const scopedWorkflows = computed(() => {
  const list = workflows.filteredWorkflows;
  if (scopeFilter.value === "global") return list.filter((workflow) => !workflow.org_id);
  if (scopeFilter.value === "org") {
    const orgId = orgs.activeOrgId;
    return orgId ? list.filter((workflow) => workflow.org_id === orgId) : list;
  }
  return list;
});

const disabledWorkflowCount = computed(() => scopedWorkflows.value.filter((workflow) => !workflow.enabled).length);
const selectedWorkflowLabel = computed(() => workflows.selectedWorkflow?.name ?? "None");

// confirm before discarding unsaved edits when switching to a different workflow.
function confirmDiscardIfDirty(): boolean {
  if (!workflows.isDirty) return true;
  return window.confirm("You have unsaved changes to this workflow. Discard them?");
}

function chooseWorkflow(workflow: (typeof scopedWorkflows.value)[number]) {
  if (workflow.id === workflows.selectedWorkflowId) return;
  if (!confirmDiscardIfDirty()) return;
  void workflows.selectWorkflow(workflow);
}

function newWorkflow() {
  if (!confirmDiscardIfDirty()) return;
  workflows.addWorkflow();
}
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

.workflow-scope-filter {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.workflow-scope-filter label {
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.workflow-scope-filter select {
  flex: 1;
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
