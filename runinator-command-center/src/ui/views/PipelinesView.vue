<template>
  <section class="pane pipelines-pane">
    <SplitPane
      class="pipeline-outer"
      storage-key="command-center.pipelines.outer"
      :initial-first-pct="24"
      :min-first="220"
      :min-second="520"
    >
      <!-- pipeline instance list -->
      <template #first>
        <div class="panel pipeline-list-panel">
          <div class="panel-toolbar">
            <div class="pipeline-copy">
              <h2>Pipelines</h2>
              <p>Named flows of chained workflows.</p>
            </div>
            <button class="btn btn-primary" @click="openNewPipeline">
              <Icon name="plus" />
              <span>New Pipeline</span>
            </button>
          </div>
          <p v-if="pipeline.error" class="pipeline-error">{{ pipeline.error }}</p>
          <ul v-if="pipeline.pipelines.length" class="pipeline-list">
            <li
              v-for="item in pipeline.pipelines"
              :key="item.id ?? item.name"
              :class="{ active: item.id === pipeline.selectedPipelineId }"
            >
              <button class="pipeline-list-item" @click="pipeline.selectPipeline(item.id)">
                <Icon name="branch" :size="14" />
                <span class="pipeline-list-name">{{ item.name }}</span>
                <span class="pipeline-list-count">{{ item.workflow_ids.length }}</span>
              </button>
            </li>
          </ul>
          <EmptyState
            v-else
            icon="branch"
            title="No pipelines yet"
            description="Create a pipeline to group workflows and the chains between them."
          >
            <button class="btn btn-primary" @click="openNewPipeline">
              <Icon name="plus" />
              <span>New Pipeline</span>
            </button>
          </EmptyState>
        </div>
      </template>

      <!-- selected pipeline canvas + inspector -->
      <template #second>
        <div v-if="!selectedPipeline" class="panel pipeline-empty-detail">
          <EmptyState
            icon="workflow"
            title="Select a pipeline"
            description="Pick a pipeline on the left, or create a new one to start drawing chains."
          />
        </div>
        <SplitPane
          v-else
          class="pipeline-inner"
          storage-key="command-center.pipelines.inner"
          :initial-first-pct="70"
          :min-first="380"
          :min-second="260"
          collapsible-second
        >
          <template #first>
            <div class="panel pipeline-canvas-panel">
              <div class="panel-toolbar">
                <div class="pipeline-copy">
                  <h2>{{ selectedPipeline.name }}</h2>
                  <p>Drag between workflows to chain them.</p>
                </div>
                <div class="pipeline-toolbar-actions">
                  <select
                    v-if="pipeline.availableWorkflows.length"
                    class="pipeline-add-select"
                    :value="''"
                    @change="onAddWorkflow"
                  >
                    <option value="" disabled>+ Add workflow…</option>
                    <option v-for="wf in pipeline.availableWorkflows" :key="wf.id" :value="wf.id">
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
              <div class="pipeline-canvas-host">
                <PipelineCanvas @open-workflow="openWorkflow" />
              </div>
            </div>
          </template>

          <template #second>
            <div class="panel pipeline-inspector">
              <template v-if="selectedEdge">
                <h3>Chain</h3>
                <p class="pipeline-inspector-summary">
                  <strong>{{ pipeline.nameById(selectedEdge.source) }}</strong>
                  →
                  <strong>{{ pipeline.nameById(selectedEdge.target) }}</strong>
                </p>
                <label class="pipeline-field">
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
                <label class="pipeline-field pipeline-field-inline">
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
              <p v-else class="pipeline-inspector-empty">
                Select a chain edge to edit it, or drag from one workflow to another to create one.
              </p>

              <div class="pipeline-members">
                <h4>Workflows in this pipeline</h4>
                <p v-if="!pipeline.memberWorkflows.length" class="hint">
                  No workflows yet. Use “Add workflow” above to add one.
                </p>
                <ul v-else>
                  <li v-for="wf in pipeline.memberWorkflows" :key="wf.id">
                    <span>{{ wf.name }}</span>
                    <button
                      class="pipeline-member-remove"
                      title="Remove from pipeline"
                      @click="wf.id && pipeline.removeWorkflowFromPipeline(wf.id)"
                    >
                      <Icon name="minus" :size="12" />
                    </button>
                  </li>
                </ul>
              </div>

              <div v-if="pipeline.unresolved.length" class="pipeline-unresolved">
                <h4>Unresolved chains</h4>
                <p class="hint">
                  These chaining triggers point at a workflow name that no longer exists.
                </p>
                <ul>
                  <li v-for="(item, index) in pipeline.unresolved" :key="index">
                    <strong>{{ item.sourceName }}</strong> → “{{ item.targetName }}” (on
                    {{ item.on }})
                  </li>
                </ul>
              </div>
            </div>
          </template>
        </SplitPane>
      </template>
    </SplitPane>

    <!-- new / settings modal -->
    <Modal v-if="nameModal.open" :title="nameModal.title" width="480px" @close="closeNameModal">
      <form class="pipeline-name-form" @submit.prevent="submitNameModal">
        <label class="pipeline-field">
          <span>Name</span>
          <input v-model="nameModal.name" type="text" placeholder="Release pipeline" autofocus />
        </label>
        <label class="pipeline-field">
          <span>Description</span>
          <input v-model="nameModal.description" type="text" placeholder="Optional" />
        </label>
      </form>

      <div v-if="nameModal.mode === 'rename'" class="pipeline-owner">
        <label class="pipeline-field">
          <span>Owning organization</span>
          <select v-model="ownerOrgId" :disabled="ownerSaving" @change="saveOwner">
            <option value="">Platform-global (none)</option>
            <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
              {{ m.org.name }}
            </option>
          </select>
        </label>
        <p class="hint">
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

    <!-- defaults modal -->
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
import type { PipelineDefaults } from "../../core/domain/models";
import type { ChainEvent } from "../../core/workflow/pipeline-graph";
import SplitPane from "../components/shared/SplitPane.vue";
import Icon from "../components/shared/Icon.vue";
import Modal from "../components/shared/Modal.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";
import PipelineDefaultsEditor from "../components/pipeline/PipelineDefaultsEditor.vue";

const pipeline = usePipelineStore();
const workflows = useWorkflowsStore();
const app = useAppStore();
const orgs = useOrgsStore();

const selectedPipeline = computed(() => pipeline.selectedPipeline);
const selectedEdge = computed(() => pipeline.selectedEdge);

const ownerOrgId = ref<string>("");
const ownerSaving = ref(false);

const nameModal = reactive({
  open: false,
  mode: "create",
  title: "New pipeline",
  name: "",
  description: "",
});
const defaultsModalOpen = ref(false);

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

<style scoped>
.pipelines-pane,
.pipeline-outer,
.pipeline-inner {
  width: 100%;
  height: 100%;
}

.pipeline-list-panel,
.pipeline-canvas-panel,
.pipeline-empty-detail {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.pipeline-canvas-host {
  flex: 1;
  min-height: 0;
}

.pipeline-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.pipeline-add-select {
  max-width: 160px;
}

.pipeline-list {
  list-style: none;
  margin: 0;
  padding: 8px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.pipeline-list li.active .pipeline-list-item {
  background: var(--surface-muted, #eef2f6);
  font-weight: 600;
}

.pipeline-list-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  background: transparent;
  border-radius: var(--radius, 6px);
  cursor: pointer;
  text-align: left;
  font-size: 13px;
  color: var(--text, #1d2939);
}

.pipeline-list-item:hover {
  background: var(--surface-muted, #f2f4f7);
}

.pipeline-list-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pipeline-list-count {
  font-size: 11px;
  color: var(--text-muted, #667085);
  background: var(--surface-muted, #eef2f6);
  border-radius: var(--radius-pill, 999px);
  padding: 1px 7px;
}

.pipeline-copy h2 {
  margin: 0;
}

.pipeline-copy p {
  margin: 2px 0 0;
  color: var(--text-muted, #475467);
  font-size: 12px;
}

.pipeline-error {
  color: var(--danger, #b42318);
  padding: 6px 12px;
  margin: 0;
}

.pipeline-inspector {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}

.pipeline-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}

.pipeline-field-inline {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.pipeline-inspector-empty,
.pipeline-inspector .hint {
  color: var(--text-muted, #475467);
  font-size: 13px;
}

.pipeline-members ul,
.pipeline-unresolved ul {
  list-style: none;
  margin: 6px 0 0;
  padding: 0;
  font-size: 13px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.pipeline-members li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.pipeline-member-remove {
  border: none;
  background: transparent;
  color: var(--text-muted, #667085);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
}

.pipeline-member-remove:hover {
  color: var(--danger, #b42318);
}

.pipeline-unresolved ul {
  padding-left: 18px;
  list-style: disc;
  font-size: 12px;
}

.pipeline-name-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.pipeline-owner {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border, #e4e7ec);
}

.pipeline-owner .hint {
  margin: 0;
  color: var(--text-muted, #475467);
  font-size: 12px;
}
</style>
