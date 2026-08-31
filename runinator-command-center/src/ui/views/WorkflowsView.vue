<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.workflows.list-split"
      :initial-first-pct="20"
      :min-first="240"
      :min-second="720"
      collapsible-first
      first-label="Workflows"
      first-icon="workflow"
      mobile-mode="toggle"
      :mobile-detail-active="mobileView === 'editor'"
    >
      <template #first>
        <div class="panel min-h-0">
          <PanelHeader
            title="Workflows"
            description="Browse definitions, select one to edit, or create a new workflow."
          >
            <button class="btn" @click="importOpen = true">
              <Icon name="upload" />
              <span>Import</span>
            </button>
            <button class="btn btn-primary" @click="openNewWorkflow">
              <Icon name="plus" />
              <span>New</span>
            </button>
          </PanelHeader>
          <div class="mb-2 flex items-center gap-2">
            <label class="text-xs uppercase tracking-wide text-fg-muted">Scope</label>
            <select v-model="scopeFilter" class="flex-1">
              <option value="all">All</option>
              <option value="org">This org</option>
              <option value="global">Global</option>
            </select>
          </div>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard label="Visible" :value="scopedWorkflows.length" />
            <MetricCard label="Disabled" :value="disabledWorkflowCount" />
            <MetricCard label="Selected" :value="selectedWorkflowLabel" />
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
            description="Workflows orchestrate tasks as a state machine. Create one to start editing on the graph and REXRAP canvas."
          >
            <button class="btn btn-primary" @click="openNewWorkflow">
              <Icon name="plus" />
              <span>New workflow</span>
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
          <template v-else>
            <BulkActionBar
              class="mb-2"
              noun="workflow"
              :count="selection.count.value"
              :actions="bulkActions"
              :busy="bulkBusy"
              @run="runBulkAction"
              @clear="selection.clear"
            />
            <DataTable>
              <thead>
                <tr>
                  <th class="w-9" scope="col">
                    <SelectCheckbox
                      :checked="selection.allSelected.value"
                      :indeterminate="selection.someSelected.value"
                      :label="selection.allSelected.value ? 'Deselect all' : 'Select all'"
                      @toggle="selection.toggleAll"
                    />
                  </th>
                  <th>Name</th>
                  <th>Version</th>
                  <th>State</th>
                </tr>
              </thead>
              <tbody>
                <template v-for="group in workflowNamespaceGroups" :key="group.namespace">
                  <tr class="bg-surface-muted text-xs font-semibold text-fg-muted">
                    <td colspan="4">
                      <button
                        class="flex w-full items-center gap-2 text-left"
                        type="button"
                        @click="toggleNamespace(group.namespace)"
                      >
                        <Icon
                          :name="
                            collapsedNamespaces.has(group.namespace)
                              ? 'chevron-right'
                              : 'arrow-down'
                          "
                        />
                        <span>{{ group.label }}</span>
                        <span class="font-normal">{{ group.workflows.length }}</span>
                      </button>
                    </td>
                  </tr>
                  <tr
                    v-for="workflow in group.workflows"
                    v-show="!collapsedNamespaces.has(group.namespace)"
                    :key="workflow.id ?? workflowPath(workflow)"
                    class="cursor-pointer"
                    :class="{
                      selected: workflows.selectedWorkflowId === workflow.id,
                      muted: !workflow.enabled,
                    }"
                    @click="chooseWorkflow(workflow)"
                  >
                    <td class="w-9">
                      <SelectCheckbox
                        :checked="selection.isSelected(workflow)"
                        :label="`Select workflow ${workflow.name}`"
                        @toggle="selection.toggle(workflow, $event)"
                      />
                    </td>
                    <td>
                      <div>{{ workflow.name }}</div>
                      <div class="text-xs text-fg-muted">
                        {{ workflowPath(workflow) }} · {{ workflow.id?.slice(0, 8) ?? "new" }}
                      </div>
                    </td>
                    <td>{{ workflow.version }}</td>
                    <td><StatusBadge :status="workflow.enabled" /></td>
                  </tr>
                </template>
              </tbody>
            </DataTable>
          </template>
        </div>
      </template>

      <template #second>
        <div class="flex h-full min-h-0 min-w-0 flex-1 flex-col">
          <MobileBackBar label="Back to workflows" @back="mobileView = 'list'" />
          <WorkflowInspector class="min-h-0 flex-1">
            <template #canvas><WorkflowCanvas /></template>
          </WorkflowInspector>
        </div>
      </template>
    </SplitPane>
    <Modal
      v-if="workflowIdentity.open"
      title="New workflow"
      description="Choose the workflow identity before opening its draft. The namespace and stable key form its durable REXRAP path."
      width="480px"
      @close="workflowIdentity.open = false"
    >
      <form
        id="workflow-identity-form"
        class="flex flex-col gap-3"
        @submit.prevent="submitNewWorkflow"
      >
        <label class="flex flex-col gap-1 text-sm">
          <span>Name</span>
          <input
            v-model.trim="workflowIdentity.name"
            type="text"
            required
            maxlength="256"
            placeholder="Release workflow"
            autofocus
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          <span>Namespace</span>
          <input
            v-model.trim="workflowIdentity.namespace"
            type="text"
            required
            maxlength="256"
            :pattern="namespacePattern"
            title="Use dot-separated identifiers, for example acme.delivery."
            placeholder="acme.delivery"
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          <span>Stable key</span>
          <input
            v-model.trim="workflowIdentity.key"
            type="text"
            required
            maxlength="256"
            :pattern="REXRAP_IDENTIFIER_PATTERN"
            title="Start with a letter or underscore and use only letters, numbers, and underscores."
            placeholder="release_train"
          />
        </label>
        <p v-if="workflowIdentityError" class="error m-0 text-xs" role="alert">
          {{ workflowIdentityError }}
        </p>
      </form>

      <template #actions>
        <button class="btn" type="button" @click="workflowIdentity.open = false">Cancel</button>
        <button
          class="btn btn-primary"
          type="submit"
          form="workflow-identity-form"
          :disabled="Boolean(workflowIdentityError)"
        >
          Create draft
        </button>
      </template>
    </Modal>
    <WorkflowStepEditorModal v-if="workflows.stepEditorOpen" />
    <WorkflowRunInputModal v-if="workflows.runInputOpen" />
    <ImportPackDialog v-if="importOpen" @close="importOpen = false" />
  </section>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import {
  artifactIdentityError,
  artifactIdentityPath,
  REXRAP_IDENTIFIER_PATTERN,
  workflowPath,
} from "../../core/domain/models";
import WorkflowCanvas from "../components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "../components/workflow/WorkflowInspector.vue";
import WorkflowStepEditorModal from "../components/workflow/WorkflowStepEditorModal.vue";
import WorkflowRunInputModal from "../components/workflow/WorkflowRunInputModal.vue";
import ImportPackDialog from "../components/workflow/ImportPackDialog.vue";
import BulkActionBar, { type BulkAction } from "../components/shared/BulkActionBar.vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import Modal from "../components/shared/Modal.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import SelectCheckbox from "../components/shared/SelectCheckbox.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useBulkSelection } from "../composables/useBulkSelection";
import { useOperationLoading } from "../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const { isLoading: loadingWorkflows, loadingMessage: loadingWorkflowsMessage } =
  useOperationLoading("Refreshing workflows");
const scopeFilter = ref<"all" | "org" | "global">("all");
const mobileView = ref<"list" | "editor">("list");
const importOpen = ref(false);
const collapsedNamespaces = ref(new Set<string>());
const workflowIdentity = reactive({ open: false, name: "", namespace: "", key: "" });
const namespacePattern = `${REXRAP_IDENTIFIER_PATTERN}(\\.${REXRAP_IDENTIFIER_PATTERN})*`;
const workflowIdentityError = computed(() => {
  const invalid = artifactIdentityError(workflowIdentity);

  if (invalid) {
    return invalid;
  }

  const path = artifactIdentityPath(workflowIdentity);
  const targetOrgId = orgs.activeOrgId ?? null;
  return workflows.workflows.some(
    (workflow) =>
      (workflow.org_id ?? null) === targetOrgId &&
      (artifactIdentityPath(workflow) === path || workflow.key === workflowIdentity.key.trim()),
  )
    ? `The stable key ${workflowIdentity.key.trim()} is already used in this scope.`
    : "";
});

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
const workflowNamespaceGroups = computed(() => {
  const groups = new Map<string, typeof scopedWorkflows.value>();

  for (const workflow of scopedWorkflows.value) {
    const namespace = workflow.namespace ?? "";
    const group = groups.get(namespace) ?? [];
    group.push(workflow);
    groups.set(namespace, group);
  }

  return [...groups.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([namespace, workflows]) => ({
      namespace,
      label: namespace || "Root",
      workflows: workflows
        .slice()
        .sort((left, right) => workflowPath(left).localeCompare(workflowPath(right))),
    }));
});

function toggleNamespace(namespace: string) {
  const next = new Set(collapsedNamespaces.value);

  if (next.has(namespace)) {
    next.delete(namespace);
  } else {
    next.add(namespace);
  }

  collapsedNamespaces.value = next;
}

// bulk selection tracks the scoped (filtered) list, so changing the scope or the search drops rows
// that are no longer visible out of the selection.
const selection = useBulkSelection(scopedWorkflows, (workflow) => workflow.id ?? workflow.name);
const bulkBusy = ref("");

const bulkActions = computed<BulkAction[]>(() => [
  {
    key: "enable",
    label: "Enable",
    icon: "check",
    disabled: selection.selectedRows.value.every((workflow) => workflow.enabled),
  },
  {
    key: "disable",
    label: "Disable",
    icon: "close",
    disabled: selection.selectedRows.value.every((workflow) => !workflow.enabled),
  },
  { key: "delete", label: "Delete", icon: "trash", variant: "danger" },
]);

async function runBulkAction(key: string) {
  const selected = selection.selectedRows.value;

  if (!selected.length || bulkBusy.value) {
    return;
  }

  // a bulk edit rewrites rows the open draft may be derived from; make the user resolve their
  // unsaved changes first rather than silently discarding or double-applying them.
  if (!confirmDiscardIfDirty()) {
    return;
  }

  bulkBusy.value = key;

  try {
    if (key === "delete") {
      await workflows.deleteWorkflows(selected);
    } else {
      await workflows.setWorkflowsEnabled(selected, key === "enable");
    }
  } finally {
    bulkBusy.value = "";
  }

  selection.clear();
}

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

function openNewWorkflow() {
  if (!confirmDiscardIfDirty()) {
    return;
  }

  workflowIdentity.name = "";
  workflowIdentity.namespace = "";
  workflowIdentity.key = "";
  workflowIdentity.open = true;
}

function submitNewWorkflow() {
  if (workflowIdentityError.value) {
    return;
  }

  mobileView.value = "editor";
  workflows.addWorkflow(
    {
      name: workflowIdentity.name,
      namespace: workflowIdentity.namespace,
      key: workflowIdentity.key,
    },
    orgs.activeOrgId ?? null,
  );
  workflowIdentity.open = false;
}
</script>
