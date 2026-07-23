<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.pipelines.outer"
      :initial-first-pct="20"
      :min-first="240"
      :min-second="520"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="mobileView === 'editor'"
    >
      <template #first>
        <div class="panel min-h-0">
          <div class="panel-toolbar">
            <div class="grid gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">Pipelines</h2>
              <p class="m-0 text-xs text-fg-muted">
                Browse named flows of chained workflows, or create a new pipeline.
              </p>
            </div>
            <button class="btn btn-primary" @click="openNewPipeline">
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
              <strong class="truncate text-sm text-fg">{{ scopedPipelines.length }}</strong>
            </div>
            <div class="metric-card">
              <span class="text-xs text-fg-muted">Workflows</span>
              <strong class="truncate text-sm text-fg">{{ memberWorkflowCount }}</strong>
            </div>
            <div class="metric-card">
              <span class="text-xs text-fg-muted">Selected</span>
              <strong class="truncate text-sm text-fg">{{ selectedPipelineLabel }}</strong>
            </div>
          </div>

          <p v-if="pipeline.error" class="error m-0 px-3 py-1.5 text-sm">{{ pipeline.error }}</p>

          <EmptyState
            v-if="!pipeline.pipelines.length"
            compact
            icon="branch"
            title="No pipelines yet"
            description="Create a pipeline to group workflows and the chains between them."
          >
            <button class="btn btn-primary" @click="openNewPipeline">
              <Icon name="plus" />
              <span>Create your first pipeline</span>
            </button>
          </EmptyState>
          <EmptyState
            v-else-if="!scopedPipelines.length"
            compact
            icon="search"
            title="No matches"
            :description="
              app.searchQuery
                ? `No pipelines match “${app.searchQuery}”.`
                : 'No pipelines match the current scope filter.'
            "
          />
          <DataTable v-else>
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Members</th>
                  <th>Scope</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="item in scopedPipelines"
                  :key="item.id ?? item.name"
                  class="cursor-pointer"
                  :class="{ selected: item.id === pipeline.selectedPipelineId }"
                  @click="choosePipeline(item)"
                >
                  <td>{{ item.name }}</td>
                  <td>{{ item.workflow_ids.length }}</td>
                  <td>{{ item.org_id ? "Org" : "Global" }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>

      <template #second>
        <div v-if="!selectedPipeline" class="panel flex h-full items-center justify-center">
          <EmptyState
            icon="workflow"
            title="Select a pipeline"
            description="Pick a pipeline on the left, or create a new one to start drawing chains."
          />
        </div>
        <div v-else class="flex h-full min-h-0 min-w-0 flex-1 flex-col">
          <MobileBackBar label="Back to pipelines" @back="mobileView = 'list'" />
          <SplitPane
            class="h-full w-full min-h-0 flex-1"
            storage-key="command-center.pipelines.inner"
            :initial-first-pct="70"
            :min-first="380"
            :min-second="260"
            collapsible-second
          >
            <template #first>
              <div class="panel h-full min-h-0">
                <div class="panel-toolbar">
                  <div class="grid gap-1">
                    <h2 class="m-0 text-base font-semibold text-fg">{{ selectedPipeline.name }}</h2>
                    <p class="m-0 text-xs text-fg-muted">Drag between workflows to chain them.</p>
                  </div>
                  <div class="flex items-center gap-1.5">
                    <select
                      v-if="pipeline.availableWorkflows.length"
                      class="max-w-40"
                      :value="''"
                      @change="onAddWorkflow"
                    >
                      <option value="" disabled>+ Add workflow…</option>
                      <option
                        v-for="wf in pipeline.availableWorkflows"
                        :key="wf.id"
                        :value="wf.id"
                      >
                        {{ wf.name }}
                      </option>
                    </select>
                    <button class="btn" @click="openDefaults">
                      <Icon name="settings" />
                      <span>Defaults</span>
                    </button>
                    <button class="btn" @click="openRename">
                      <Icon name="edit" />
                      <span>Settings</span>
                    </button>
                    <button class="btn btn-danger" @click="confirmDelete">
                      <Icon name="trash" />
                    </button>
                  </div>
                </div>
                <div class="min-h-0 flex-1">
                  <PipelineCanvas @open-workflow="openWorkflow" />
                </div>
              </div>
            </template>

            <template #second>
              <div class="panel gap-3 overflow-y-auto p-4">
                <template v-if="selectedEdge">
                  <h3 class="m-0 text-sm font-semibold text-fg">Chain</h3>
                  <p class="m-0 text-sm text-fg">
                    <strong>{{ pipeline.nameById(selectedEdge.source) }}</strong>
                    →
                    <strong>{{ pipeline.nameById(selectedEdge.target) }}</strong>
                  </p>
                  <label class="flex flex-col gap-1 text-sm">
                    <span>Fires on</span>
                    <select
                      :value="selectedEdge.data.on"
                      @change="onSelectorChange(($event.target as HTMLSelectElement).value)"
                    >
                      <option value="success">Success</option>
                      <option value="failure">Failure</option>
                      <option value="complete">Complete</option>
                    </select>
                  </label>
                  <label class="flex flex-row items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      :checked="selectedEdge.data.enabled"
                      @change="onEnabledChange(($event.target as HTMLInputElement).checked)"
                    />
                    <span>Enabled</span>
                  </label>
                  <button class="btn btn-danger" @click="pipeline.deleteSelected">
                    <Icon name="trash" />
                    <span>Delete chain</span>
                  </button>
                </template>
                <p v-else class="m-0 text-sm text-fg-muted">
                  Select a chain edge to edit it, or drag from one workflow to another to create one.
                </p>

                <div>
                  <h4 class="m-0 text-sm font-semibold text-fg">Workflows in this pipeline</h4>
                  <p v-if="!pipeline.memberWorkflows.length" class="hint mt-1.5">
                    No workflows yet. Use “Add workflow” above to add one.
                  </p>
                  <ul v-else class="mt-1.5 flex list-none flex-col gap-1 p-0 text-sm">
                    <li
                      v-for="wf in pipeline.memberWorkflows"
                      :key="wf.id"
                      class="flex items-center justify-between gap-2"
                    >
                      <span>{{ wf.name }}</span>
                      <button
                        class="inline-flex items-center border-0 bg-transparent text-fg-muted hover:text-danger-fg"
                        title="Remove from pipeline"
                        @click="wf.id && pipeline.removeWorkflowFromPipeline(wf.id)"
                      >
                        <Icon name="minus" :size="12" />
                      </button>
                    </li>
                  </ul>
                </div>

                <div v-if="pipeline.unresolved.length">
                  <h4 class="m-0 text-sm font-semibold text-fg">Unresolved chains</h4>
                  <p class="hint mt-1.5">
                    These chaining triggers point at a workflow name that no longer exists.
                  </p>
                  <ul class="mt-1.5 list-disc pl-4 text-xs text-fg">
                    <li v-for="(item, index) in pipeline.unresolved" :key="index">
                      <strong>{{ item.sourceName }}</strong> → “{{ item.targetName }}” (on
                      {{ item.on }})
                    </li>
                  </ul>
                </div>
              </div>
            </template>
          </SplitPane>
        </div>
      </template>
    </SplitPane>

    <Modal v-if="nameModal.open" :title="nameModal.title" width="480px" @close="closeNameModal">
      <form class="flex flex-col gap-3" @submit.prevent="submitNameModal">
        <label class="flex flex-col gap-1 text-sm">
          <span>Name</span>
          <input v-model="nameModal.name" type="text" placeholder="Release pipeline" autofocus />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          <span>Description</span>
          <input v-model="nameModal.description" type="text" placeholder="Optional" />
        </label>
      </form>

      <div
        v-if="nameModal.mode === 'rename'"
        class="mt-3 flex flex-col gap-2 border-t border-border pt-3"
      >
        <label class="flex flex-col gap-1 text-sm">
          <span>Owning organization</span>
          <select v-model="ownerOrgId" :disabled="ownerSaving" @change="saveOwner">
            <option value="">Platform-global (none)</option>
            <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
              {{ m.org.name }}
            </option>
          </select>
        </label>
        <p class="hint m-0">
          Scoping a pipeline to an org limits its visibility to that org's members. Only org admins
          can move a pipeline into an org.
        </p>
      </div>

      <template #actions>
        <button class="btn" @click="closeNameModal">Cancel</button>
        <button class="btn btn-primary" :disabled="!nameModal.name.trim()" @click="submitNameModal">
          {{ nameModal.mode === "create" ? "Create" : "Save" }}
        </button>
      </template>
    </Modal>

    <Modal
      v-if="defaultsModalOpen && selectedPipeline"
      title="Pipeline defaults"
      width="560px"
      @close="defaultsModalOpen = false"
    >
      <PipelineDefaultsEditor
        :defaults="selectedPipeline.defaults"
        @cancel="defaultsModalOpen = false"
        @save="submitDefaults"
      />
    </Modal>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { usePipelineStore } from "../adapters/pinia/pipeline";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import { useAppStore } from "../adapters/pinia/app";
import { useOrgsStore } from "../adapters/pinia/orgs";
import type { Pipeline, PipelineDefaults } from "../../core/domain/models";
import type { ChainEvent } from "../../core/workflow/pipeline-graph";
import SplitPane from "../components/shared/SplitPane.vue";
import Icon from "../components/shared/Icon.vue";
import Modal from "../components/shared/Modal.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import DataTable from "../components/shared/DataTable.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";
import PipelineDefaultsEditor from "../components/pipeline/PipelineDefaultsEditor.vue";

const pipeline = usePipelineStore();
const workflows = useWorkflowsStore();
const app = useAppStore();
const orgs = useOrgsStore();

const selectedPipeline = computed(() => pipeline.selectedPipeline);
const selectedEdge = computed(() => pipeline.selectedEdge);
const scopeFilter = ref<"all" | "org" | "global">("all");
const mobileView = ref<"list" | "editor">("list");

const scopedPipelines = computed(() => {
  const query = app.searchQuery.trim().toLowerCase();
  let list = pipeline.pipelines;

  if (scopeFilter.value === "global") {
    list = list.filter((item) => !item.org_id);
  } else if (scopeFilter.value === "org") {
    const orgId = orgs.activeOrgId;
    list = orgId ? list.filter((item) => item.org_id === orgId) : list;
  }

  if (!query) {
    return list;
  }

  return list.filter(
    (item) =>
      item.name.toLowerCase().includes(query) ||
      (item.description ?? "").toLowerCase().includes(query),
  );
});

const memberWorkflowCount = computed(
  () => new Set(scopedPipelines.value.flatMap((item) => item.workflow_ids)).size,
);
const selectedPipelineLabel = computed(() => selectedPipeline.value?.name ?? "None");

const ownerOrgId = ref<string>("");
const ownerSaving = ref(false);

const nameModal = reactive({
  open: false,
  mode: "create" as "create" | "rename",
  title: "New pipeline",
  name: "",
  description: "",
});
const defaultsModalOpen = ref(false);

function choosePipeline(item: Pipeline) {
  if (item.id === pipeline.selectedPipelineId) {
    return;
  }

  mobileView.value = "editor";
  void pipeline.selectPipeline(item.id);
}

function openNewPipeline() {
  nameModal.open = true;
  nameModal.mode = "create";
  nameModal.title = "New pipeline";
  nameModal.name = "";
  nameModal.description = "";
}

function openRename() {
  const current = selectedPipeline.value;

  if (!current) {
    return;
  }

  nameModal.open = true;
  nameModal.mode = "rename";
  nameModal.title = "Pipeline settings";
  nameModal.name = current.name;
  nameModal.description = current.description ?? "";
  ownerOrgId.value = current.org_id ?? "";

  if (!orgs.memberships.length) {
    void orgs.refresh();
  }
}

async function saveOwner() {
  ownerSaving.value = true;

  try {
    await pipeline.setPipelineOwner(ownerOrgId.value || null);
    ownerOrgId.value = selectedPipeline.value?.org_id ?? "";
    app.setStatus("Pipeline ownership updated");
  } finally {
    ownerSaving.value = false;
  }
}

function closeNameModal() {
  nameModal.open = false;
}

async function submitNameModal() {
  if (!nameModal.name.trim()) {
    return;
  }

  if (nameModal.mode === "create") {
    await pipeline.createPipeline(nameModal.name, nameModal.description);
    mobileView.value = "editor";
  } else {
    await pipeline.renamePipeline(nameModal.name, nameModal.description.trim() || null);
  }

  nameModal.open = false;
}

function openDefaults() {
  defaultsModalOpen.value = true;
}

async function submitDefaults(defaults: PipelineDefaults) {
  await pipeline.savePipelineDefaults(defaults);
  defaultsModalOpen.value = false;
}

async function confirmDelete() {
  const current = selectedPipeline.value;

  if (!current?.id) {
    return;
  }

  if (window.confirm(`Delete pipeline “${current.name}”? Chained links stay on the workflows.`)) {
    await pipeline.deletePipeline(current.id);
    mobileView.value = "list";
  }
}

function onAddWorkflow(event: Event) {
  const select = event.target as HTMLSelectElement;
  const workflowId = select.value;
  select.value = "";

  if (workflowId) {
    void pipeline.addWorkflowToPipeline(workflowId);
  }
}

function onSelectorChange(value: string) {
  void pipeline.updateSelected({ on: value as ChainEvent });
}

function onEnabledChange(enabled: boolean) {
  void pipeline.updateSelected({ enabled });
}

function openWorkflow(workflowId: string) {
  const workflow = workflows.workflows.find((wf) => wf.id === workflowId);

  if (workflow) {
    void workflows.selectWorkflow(workflow);
    app.activeTab = "Workflows";
  }
}

onMounted(() => {
  void pipeline.refresh();
});
</script>
