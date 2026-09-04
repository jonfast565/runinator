<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.pipelines.outer"
      :initial-first-pct="20"
      :min-first="240"
      :min-second="520"
      collapsible-first
      first-label="Pipelines"
      first-icon="branch"
      mobile-mode="toggle"
      :mobile-detail-active="mobileView === 'editor'"
    >
      <template #first>
        <div class="panel min-h-0">
          <PanelHeader
            title="Pipelines"
            icon="branch"
            eyebrow="Workflow composition"
            description="Browse named flows of chained workflows, or create a new pipeline."
          >
            <button class="btn btn-primary" @click="openNewPipeline">
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
            <MetricCard label="Visible" :value="scopedPipelines.length" />
            <MetricCard label="Workflows" :value="memberWorkflowCount" />
            <MetricCard label="Selected" :value="selectedPipelineLabel" />
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
              <span>New pipeline</span>
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
          <DataTable v-else table-class="entity-banner-table table-resize-disabled">
            <thead>
              <tr>
                <th>Pipeline</th>
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
                <td :title="`${item.name}\n${pipelinePath(item)}`">
                  <div class="entity-banner-content">
                    <span class="entity-banner-title">{{ item.name }}</span>
                    <span class="entity-banner-meta">
                      {{ pipelinePath(item) }} · {{ item.graph.members.length }} member{{
                        item.graph.members.length === 1 ? "" : "s"
                      }}
                      · {{ item.org_id ? "Organization" : "Global" }}
                    </span>
                  </div>
                </td>
              </tr>
            </tbody>
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
            second-label="Details"
            second-icon="info"
          >
            <template #first>
              <div class="panel h-full min-h-0">
                <PanelHeader
                  :title="pipelinePath(selectedPipeline)"
                  :description="`${selectedPipeline.name} · Drag between workflows to chain them.`"
                >
                  <select
                    v-if="pipeline.availableWorkflows.length"
                    class="max-w-40"
                    :value="''"
                    @change="onAddWorkflow"
                  >
                    <option value="" disabled>+ Add workflow…</option>
                    <option v-for="wf in pipeline.availableWorkflows" :key="wf.id" :value="wf.id">
                      {{ wf.name }}
                    </option>
                  </select>
                  <button
                    class="btn btn-primary"
                    :disabled="starting || !selectedPipeline.enabled"
                    @click="startRun"
                  >
                    <Icon name="runs" />
                    <span>Run</span>
                  </button>
                  <button class="btn" @click="togglePipelineEnabled">
                    <Icon :name="selectedPipeline.enabled ? 'pause' : 'play'" />
                    <span>{{ selectedPipeline.enabled ? "Disable" : "Enable" }}</span>
                  </button>
                  <button class="btn" @click="openDefaults">
                    <Icon name="settings" />
                    <span>Defaults</span>
                  </button>
                  <button class="btn" @click="openOrchestration">
                    <Icon name="branch" />
                    <span>Orchestration</span>
                  </button>
                  <button class="btn" @click="openRename">
                    <Icon name="edit" />
                    <span>Settings</span>
                  </button>
                  <button class="btn btn-danger" @click="confirmDelete">
                    <Icon name="trash" />
                  </button>
                </PanelHeader>
                <div class="min-h-0 flex-1">
                  <PipelineCanvas @open-workflow="openWorkflow" />
                </div>
              </div>
            </template>

            <template #second>
              <div class="panel gap-3 overflow-y-auto p-4">
                <template v-if="selectedEdge">
                  <div class="flex items-center gap-1">
                    <h3 class="m-0 text-sm font-semibold text-fg">Chain</h3>
                    <HelpBubble
                      text="Configure when this chain fires and how upstream output maps into the downstream workflow input."
                      label="About pipeline chains"
                    />
                  </div>
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
                  <div class="flex flex-col gap-1 text-sm">
                    <span>Downstream input mapping</span>
                    <JsonEditor v-model="edgeParametersText" title="Link mapping" />
                    <p v-if="mappingError" class="error m-0 text-xs">{{ mappingError }}</p>
                    <button
                      class="btn btn-sm"
                      :disabled="Boolean(mappingError)"
                      @click="saveEdgeMapping"
                    >
                      Save mapping
                    </button>
                  </div>
                  <button class="btn btn-danger" @click="pipeline.deleteSelected">
                    <Icon name="trash" />
                    <span>Delete chain</span>
                  </button>
                </template>
                <template v-else-if="selectedNode">
                  <h3 class="m-0 text-sm font-semibold text-fg">{{ selectedNode.data.name }}</h3>
                  <label class="flex flex-col gap-1 text-sm">
                    <span>On failure</span>
                    <select
                      :value="memberFailureModeValue"
                      @change="
                        onMemberFailureModeChange(($event.target as HTMLSelectElement).value)
                      "
                    >
                      <option value="">Pipeline default ({{ defaultFailureModeLabel }})</option>
                      <option value="stop">Stop</option>
                      <option value="continue">Continue</option>
                      <option value="silently_continue">Silently Continue</option>
                      <option value="inquire">Inquire</option>
                    </select>
                  </label>
                  <HelpBubble
                    text="Controls what happens to this pipeline run if the selected workflow fails. It mirrors PowerShell's ErrorActionPreference."
                    label="About member failure handling"
                  />
                  <div
                    v-if="selectedNodeJoin"
                    class="grid gap-2 border-t border-border-subtle pt-3"
                  >
                    <h4 class="m-0 text-sm font-semibold text-fg">Join</h4>
                    <label class="flex flex-col gap-1 text-sm">
                      <span>Readiness</span>
                      <select v-model="joinMode">
                        <option value="all">All inputs</option>
                        <option value="any">Any input</option>
                        <option value="first_success">First success</option>
                      </select>
                    </label>
                    <JsonEditor v-model="joinParametersText" title="Join mapping" />
                    <p v-if="joinMappingError" class="error m-0 text-xs">{{ joinMappingError }}</p>
                    <button
                      class="btn btn-sm"
                      :disabled="Boolean(joinMappingError)"
                      @click="saveJoin"
                    >
                      Save join
                    </button>
                  </div>
                </template>
                <div v-else />

                <div>
                  <div class="flex items-center gap-1">
                    <h4 class="m-0 text-sm font-semibold text-fg">Workflows in this pipeline</h4>
                    <HelpBubble
                      v-if="!pipeline.memberWorkflows.length"
                      text="Use Add workflow above to add the first member."
                      label="How to add a workflow"
                    />
                  </div>
                  <p v-if="!pipeline.memberWorkflows.length" class="hint mt-1.5">
                    No workflows yet.
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
                  <div class="flex items-center gap-1">
                    <h4 class="m-0 text-sm font-semibold text-fg">Unresolved chains</h4>
                    <HelpBubble
                      text="These chaining triggers point at a workflow name that no longer exists."
                      label="About unresolved chains"
                    />
                  </div>
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

    <Modal
      v-if="nameModal.open"
      :title="nameModal.title"
      description="Name and scope identify the pipeline; its stable key is used for durable references."
      width="480px"
      @close="closeNameModal"
    >
      <form
        id="pipeline-identity-form"
        class="flex flex-col gap-3"
        @submit.prevent="submitNameModal"
      >
        <label class="flex flex-col gap-1 text-sm">
          <span>Name</span>
          <input
            v-model.trim="nameModal.name"
            type="text"
            required
            maxlength="256"
            placeholder="Release pipeline"
            autofocus
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          <span>Namespace</span>
          <input
            v-model.trim="nameModal.namespace"
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
            v-model.trim="nameModal.key"
            type="text"
            required
            maxlength="256"
            :pattern="REXRAP_IDENTIFIER_PATTERN"
            title="Start with a letter or underscore and use only letters, numbers, and underscores."
            placeholder="release_train"
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          <span>Description</span>
          <input
            v-model="nameModal.description"
            type="text"
            maxlength="240"
            placeholder="Optional"
          />
        </label>
        <p
          v-if="pipelineIdentityError || identitySubmitError"
          class="error m-0 text-xs"
          role="alert"
        >
          {{ pipelineIdentityError || identitySubmitError }}
        </p>
      </form>

      <div
        v-if="nameModal.mode === 'rename'"
        class="mt-3 flex flex-col gap-2 border-t border-border pt-3"
      >
        <label class="flex flex-col gap-1 text-sm">
          <span class="flex items-center gap-1"
            >Owning organization
            <HelpBubble
              text="Scoping a pipeline to an organization limits visibility to its members. Only organization admins can move a pipeline into an organization."
              label="About pipeline ownership"
          /></span>
          <select v-model="ownerOrgId" :disabled="ownerSaving" @change="saveOwner">
            <option value="">Platform-global (none)</option>
            <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
              {{ m.org.name }}
            </option>
          </select>
        </label>
      </div>

      <template #actions>
        <button class="btn" type="button" @click="closeNameModal">Cancel</button>
        <button
          class="btn btn-primary"
          type="submit"
          form="pipeline-identity-form"
          :disabled="!validPipelineIdentity || identitySaving"
        >
          {{ identitySaving ? "Saving…" : nameModal.mode === "create" ? "Create" : "Save" }}
        </button>
      </template>
    </Modal>

    <Modal
      v-if="defaultsModalOpen && selectedPipeline"
      title="Pipeline defaults"
      description="Set the pipeline-wide failure and concurrency behavior inherited by its members."
      width="560px"
      @close="defaultsModalOpen = false"
    >
      <PipelineDefaultsEditor
        :defaults="selectedPipeline.defaults"
        :concurrency="selectedPipeline.concurrency"
        @cancel="defaultsModalOpen = false"
        @save="submitDefaults"
      />
    </Modal>

    <Modal
      v-if="orchestrationModalOpen && selectedPipeline"
      title="Pipeline orchestration"
      description="Configure how correlated external events observe, pause, restart, or signal pipeline runs."
      width="min(1100px, 96vw)"
      @close="orchestrationModalOpen = false"
    >
      <PipelineOrchestrationEditor
        :pipeline="selectedPipeline"
        :adapter-kinds="adapterKinds"
        @cancel="orchestrationModalOpen = false"
        @save="submitOrchestration"
      />
    </Modal>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, shallowRef, watch } from "vue";
import { usePipelineStore } from "../adapters/pinia/pipeline";
import { usePipelineRunsStore } from "../adapters/pinia/pipeline-runs";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import { useAppStore } from "../adapters/pinia/app";
import { useOrgsStore } from "../adapters/pinia/orgs";
import type {
  Pipeline,
  PipelineDefaults,
  PipelineConcurrency,
  PipelineJoinMode,
  PipelineMemberFailureMode,
  AdapterKindMetadata,
  JsonRecord,
} from "../../core/domain/models";
import {
  artifactIdentityError,
  artifactIdentityPath,
  pipelinePath,
  REXRAP_IDENTIFIER_PATTERN,
} from "../../core/domain/models";
import { fetchAdapterKinds } from "../../core/services/orchestrations";
import type { ChainEvent } from "../../core/workflow/pipeline-graph";
import SplitPane from "../components/shared/SplitPane.vue";
import Icon from "../components/shared/Icon.vue";
import Modal from "../components/shared/Modal.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import HelpBubble from "../components/shared/HelpBubble.vue";
import DataTable from "../components/shared/DataTable.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";
import PipelineDefaultsEditor from "../components/pipeline/PipelineDefaultsEditor.vue";
import PipelineOrchestrationEditor from "../components/pipeline/PipelineOrchestrationEditor.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";

const pipeline = usePipelineStore();
const pipelineRuns = usePipelineRunsStore();
const workflows = useWorkflowsStore();
const app = useAppStore();
const orgs = useOrgsStore();

const selectedPipeline = computed(() => pipeline.selectedPipeline);
const selectedEdge = computed(() => pipeline.selectedEdge);
const selectedNode = computed(() => pipeline.selectedNode);
const selectedNodeJoin = computed(() => {
  const member = selectedPipeline.value?.graph.members.find(
    (item) => item.workflow_id === selectedNode.value?.data.workflowId,
  );
  return member ? (selectedPipeline.value?.graph.joins[member.key] ?? null) : null;
});
const edgeParametersText = ref("{}");
const mappingError = ref<string | null>(null);
const joinMode = ref<PipelineJoinMode>("all");
const joinParametersText = ref("{}");
const joinMappingError = ref<string | null>(null);

watch(
  selectedEdge,
  (edge) => {
    edgeParametersText.value = JSON.stringify(edge?.data.parameters ?? {}, null, 2);
    mappingError.value = null;
  },
  { immediate: true },
);
watch(
  selectedNodeJoin,
  (join) => {
    joinMode.value = join?.mode ?? "all";
    joinParametersText.value = JSON.stringify(join?.parameters ?? {}, null, 2);
    joinMappingError.value = null;
  },
  { immediate: true },
);

function parseMapping(
  text: string,
  setError: (message: string | null) => void,
): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(text || "{}") as unknown;

    if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("Mapping must be a JSON object.");
    }

    setError(null);
    return parsed as Record<string, unknown>;
  } catch (error) {
    setError(error instanceof Error ? error.message : String(error));
    return null;
  }
}

function saveEdgeMapping() {
  const parameters = parseMapping(edgeParametersText.value, (message) => {
    mappingError.value = message;
  });

  if (parameters) {
    void pipeline.updateSelected({ parameters });
  }
}

function saveJoin() {
  const join = selectedNodeJoin.value;
  const parameters = parseMapping(joinParametersText.value, (message) => {
    joinMappingError.value = message;
  });

  if (join && parameters) {
    void pipeline.updateJoin(join.target, joinMode.value, parameters);
  }
}

// "" means no per-member override (the pipeline default applies).
const memberFailureModeValue = computed(() => {
  const node = selectedNode.value;
  const member = selectedPipeline.value?.graph.members.find(
    (item) => item.workflow_id === node?.data.workflowId,
  );
  return member?.failure_mode ?? "";
});

const defaultFailureModeLabel = computed(
  () => selectedPipeline.value?.defaults.default_failure_mode ?? "continue",
);

function onMemberFailureModeChange(value: string) {
  const node = selectedNode.value;

  if (!node) {
    return;
  }

  void pipeline.setMemberFailureMode(
    node.data.workflowId,
    (value || null) as PipelineMemberFailureMode | null,
  );
}

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
      pipelinePath(item).toLowerCase().includes(query) ||
      (item.description ?? "").toLowerCase().includes(query),
  );
});

const memberWorkflowCount = computed(
  () =>
    new Set(
      scopedPipelines.value.flatMap((item) =>
        item.graph.members.map((member) => member.workflow_id),
      ),
    ).size,
);
const selectedPipelineLabel = computed(() =>
  selectedPipeline.value ? pipelinePath(selectedPipeline.value) : "None",
);

const ownerOrgId = ref<string>("");
const ownerSaving = ref(false);

const nameModal = reactive({
  open: false,
  mode: "create",
  title: "New pipeline",
  name: "",
  namespace: "",
  key: "",
  description: "",
});
const namespacePattern = `${REXRAP_IDENTIFIER_PATTERN}(\\.${REXRAP_IDENTIFIER_PATTERN})*`;
const identitySubmitError = ref("");
const identitySaving = ref(false);
const pipelineIdentityError = computed(() => {
  const invalid = artifactIdentityError(nameModal);

  if (invalid) {
    return invalid;
  }

  const path = artifactIdentityPath(nameModal);
  const selectedId = nameModal.mode === "rename" ? selectedPipeline.value?.id : null;
  const targetOrgId =
    nameModal.mode === "rename"
      ? (selectedPipeline.value?.org_id ?? null)
      : (orgs.activeOrgId ?? null);
  return pipeline.pipelines.some(
    (candidate) =>
      candidate.id !== selectedId &&
      (candidate.org_id ?? null) === targetOrgId &&
      (artifactIdentityPath(candidate) === path || candidate.key === nameModal.key.trim()),
  )
    ? `The stable key ${nameModal.key.trim()} is already used in this scope.`
    : "";
});
const validPipelineIdentity = computed(() => !pipelineIdentityError.value);
const defaultsModalOpen = ref(false);
const orchestrationModalOpen = ref(false);
const adapterKinds = shallowRef<AdapterKindMetadata[]>([]);
const starting = ref(false);

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
  nameModal.namespace = "";
  nameModal.key = "";
  nameModal.description = "";
  identitySubmitError.value = "";
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
  nameModal.namespace = current.namespace ?? "";
  nameModal.key = current.key ?? "";
  nameModal.description = current.description ?? "";
  identitySubmitError.value = "";
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
  if (!validPipelineIdentity.value || identitySaving.value) {
    return;
  }

  identitySubmitError.value = "";
  identitySaving.value = true;

  try {
    if (nameModal.mode === "create") {
      const saved = await pipeline.createPipeline(
        nameModal.name,
        nameModal.namespace,
        nameModal.key,
        nameModal.description,
      );

      if (!saved) {
        identitySubmitError.value = pipeline.error ?? "Could not create pipeline.";
        return;
      }

      mobileView.value = "editor";
    } else {
      const saved = await pipeline.renamePipeline(
        nameModal.name,
        nameModal.description.trim() || null,
        nameModal.namespace,
        nameModal.key,
      );

      if (!saved) {
        identitySubmitError.value = pipeline.error ?? "Could not save pipeline settings.";
        return;
      }
    }

    nameModal.open = false;
  } finally {
    identitySaving.value = false;
  }
}

// start the selected pipeline and hand off to the pipeline-runs monitor with the new run selected.
async function startRun() {
  const current = selectedPipeline.value;

  if (!current?.id || starting.value) {
    return;
  }

  starting.value = true;

  try {
    await pipelineRuns.startRun(current.id);
    app.activeTab = "PipelineRuns";
    app.setStatus(`Started ${current.name}`);
  } catch (error) {
    app.setError(error instanceof Error ? error.message : String(error));
  } finally {
    starting.value = false;
  }
}

async function togglePipelineEnabled() {
  const current = selectedPipeline.value;

  if (!current) {
    return;
  }

  const enabled = !current.enabled;

  if (await pipeline.setPipelineEnabled(enabled)) {
    app.setStatus(`${current.name} ${enabled ? "enabled" : "disabled"}`);
  }
}

function openDefaults() {
  defaultsModalOpen.value = true;
}

async function openOrchestration() {
  try {
    adapterKinds.value = (await fetchAdapterKinds())
      .filter((entry) => entry.healthy && !entry.error)
      .map((entry) => entry.metadata);
  } catch (error) {
    adapterKinds.value = [];
    app.setError(error instanceof Error ? error.message : String(error));
  }

  orchestrationModalOpen.value = true;
}

async function submitDefaults(defaults: PipelineDefaults, concurrency: PipelineConcurrency) {
  await pipeline.savePipelineDefaults(defaults);
  await pipeline.savePipelineConcurrency(concurrency);
  defaultsModalOpen.value = false;
}

async function submitOrchestration(metadata: JsonRecord) {
  await pipeline.savePipelineMetadata(metadata);
  orchestrationModalOpen.value = false;
}

async function confirmDelete() {
  const current = selectedPipeline.value;

  if (!current?.id) {
    return;
  }

  if (window.confirm(`Delete pipeline “${current.name}” and its graph?`)) {
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
