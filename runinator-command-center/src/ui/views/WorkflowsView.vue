<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.workflows.list-split"
      :initial-first-pct="20"
      :min-first="240"
      :min-second="720"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="mobileView === 'editor'"
    >
      <template #first>
        <div class="panel min-h-0">
          <div class="panel-toolbar">
            <div class="grid gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">Workflows</h2>
              <p class="m-0 text-xs text-fg-muted">
                Browse definitions, select one to edit, or create a new workflow.
              </p>
            </div>
            <button class="btn btn-primary" @click="newWorkflow">
              <Icon name="plus" />
              <span>New</span>
            </button>
          </div>
          <div class="mb-2 flex items-center gap-2">
            <label class="text-xs uppercase tracking-wide text-fg-muted">Scope</label>
            <select v-model="scopeFilter" class="flex-1">
              <option value="all">All</option>
              <option value="org">This org</option>
              <option value="global">Global</option>
            </select>
          </div>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <div class="metric-card">
              <span class="text-xs text-fg-muted">Visible</span>
              <strong class="truncate text-sm text-fg">{{ scopedWorkflows.length }}</strong>
            </div>
            <div class="metric-card">
              <span class="text-xs text-fg-muted">Disabled</span>
              <strong class="truncate text-sm text-fg">{{ disabledWorkflowCount }}</strong>
            </div>
            <div class="metric-card">
              <span class="text-xs text-fg-muted">Selected</span>
              <strong class="truncate text-sm text-fg">{{ selectedWorkflowLabel }}</strong>
            </div>
          </div>
          <EmptyState
            v-if="loadingWorkflows"
            compact
            loading
            title="Loading workflows"
            :loading-message="loadingWorkflowsMessage"
          />
          <EmptyState
            v-else-if="!workflows.workflows.length"
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
            :description="
              app.searchQuery
                ? `No workflows match “${app.searchQuery}”.`
                : 'No workflows match the current scope filter.'
            "
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
                  class="cursor-pointer"
                  :class="{
                    selected: workflows.selectedWorkflowId === workflow.id,
                    muted: !workflow.enabled,
                  }"
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
        <div class="flex h-full min-h-0 min-w-0 flex-1 flex-col">
          <MobileBackBar label="Back to workflows" @back="mobileView = 'list'" />
          <SplitPane
            class="min-h-0 flex-1"
            storage-key="command-center.workflows.inspector-split"
            :initial-first-pct="64"
            :min-first="360"
            :min-second="320"
            collapsible-second
            mobile-mode="toggle"
            :mobile-detail-active="!!workflows.selectedStepId"
          >
            <template #first>
              <WorkflowCanvas />
            </template>
            <template #second>
              <div class="flex h-full min-h-0 min-w-0 flex-1 flex-col">
                <MobileBackBar label="Back to canvas" @back="workflows.selectedStepId = ''" />
                <WorkflowInspector class="min-h-0 flex-1" />
              </div>
            </template>
          </SplitPane>
        </div>
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
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOperationLoading } from "../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const { isLoading: loadingWorkflows, loadingMessage: loadingWorkflowsMessage } =
  useOperationLoading("Refreshing workflows");
const scopeFilter = ref<"all" | "org" | "global">("all");
const mobileView = ref<"list" | "editor">("list");

const scopedWorkflows = computed(() => {
  const list = workflows.filteredWorkflows;

  if (scopeFilter.value === "global") {
    return list.filter((workflow) => !workflow.org_id);
  }

  if (scopeFilter.value === "org") {
    const orgId = orgs.activeOrgId;
    return orgId ? list.filter((workflow) => workflow.org_id === orgId) : list;
  }

  return list;
});

const disabledWorkflowCount = computed(
  () => scopedWorkflows.value.filter((workflow) => !workflow.enabled).length,
);
const selectedWorkflowLabel = computed(() => workflows.selectedWorkflow?.name ?? "None");

function confirmDiscardIfDirty(): boolean {
  if (!workflows.isDirty) {
    return true;
  }

  return window.confirm("You have unsaved changes to this workflow. Discard them?");
}

function chooseWorkflow(workflow: (typeof scopedWorkflows.value)[number]) {
  if (workflow.id === workflows.selectedWorkflowId) {
    return;
  }

  if (!confirmDiscardIfDirty()) {
    return;
  }

  mobileView.value = "editor";
  void workflows.selectWorkflow(workflow);
}

function newWorkflow() {
  if (!confirmDiscardIfDirty()) {
    return;
  }

  mobileView.value = "editor";
  workflows.addWorkflow();
}
</script>
