<template>
  <div
    ref="modalRoot"
    class="modal-backdrop"
    tabindex="-1"
    @keydown.esc.stop.prevent="workflows.closeStepEditor"
  >
    <form class="modal step-modal" @submit.prevent="workflows.submitStepEditor">
      <header class="modal-header">
        <div>
          <h2>{{ workflows.stepEditorCreating ? "Add Workflow Step" : "Edit Workflow Step" }}</h2>
          <span>{{ workflows.selectedStepId || "New step" }}</span>
        </div>
        <button
          type="button"
          class="btn-close"
          aria-label="Close"
          @click="workflows.closeStepEditor"
        >
          <Icon name="close" :size="16" />
        </button>
      </header>

      <section class="form-section">
        <h3>Step</h3>
        <div class="form-grid">
          <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
          <label
            >Name
            <input
              v-model="workflows.stepEditor.name"
              placeholder="Shown on the node; defaults to the step ID"
          /></label>
          <label>
            Node Kind
            <select
              v-model="workflows.stepEditor.kind"
              :disabled="workflows.selectedStepKindLocked"
              @change="onKindChange"
            >
              <option value="start">start</option>
              <option v-for="kind in workflows.workflowNodeKinds" :key="kind" :value="kind">
                {{ workflowNodeKindLabel(kind) }}
              </option>
              <option value="end">end</option>
              <option value="fail">fail</option>
            </select>
          </label>
        </div>
      </section>

      <section class="form-section">
        <h3>Runtime</h3>
        <div class="form-grid runtime-grid">
          <label class="checkbox">
            <input
              v-model="workflows.stepEditor.locked"
              type="checkbox"
              :disabled="isProtectedNode"
            />
            Locked
          </label>
          <label class="checkbox">
            <input v-model="workflows.stepEditor.skipped" type="checkbox" />
            Skipped
          </label>
          <label
            >Max Attempts
            <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1"
          /></label>
          <label
            >Timeout Seconds
            <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0"
          /></label>
        </div>
      </section>

      <!-- catalog-driven parameter fields for this node kind. -->
      <section v-if="kindMetadata && kindMetadata.fields.length" class="form-section">
        <h3>Parameters</h3>
        <CatalogFieldEditor
          v-for="field in kindMetadata.fields"
          :key="field.name"
          :field="field"
          :model-value="fieldValue(field)"
          :expression-context="expressionContext"
          :node-options="nodeIdOptions"
          :workflows="availableSubflows"
          :sibling-values="actionSiblingValues"
          @update:model-value="setFieldValue(field, $event)"
        />
      </section>
      <section
        v-else-if="!kindMetadata && !isProtectedNode"
        class="form-section catalog-loading-section"
      >
        <h3>Parameters</h3>
        <p class="hint catalog-loading-inline">
          <LoadingSpinner size="sm" label="Loading node metadata" />
          Loading node metadata…
        </p>
      </section>

      <!-- action configuration via TypedParameterEditor when a provider action is selected. -->
      <section
        v-if="workflows.stepEditor.kind === 'action' && selectedAction"
        class="form-section"
      >
        <h3>Action Parameters</h3>
        <TypedParameterEditor
          v-if="selectedAction.parameters?.length"
          v-model="actionConfiguration"
          :parameters="selectedAction.parameters"
          :credential-scopes="currentProvider?.metadata.credential_scopes ?? []"
          :expression-context="expressionContext"
        />
        <KeyValueObjectEditor
          v-else
          v-model="actionConfiguration"
          title="Action Parameters"
          empty-label="No action parameters configured."
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="actionConfigurationJson"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <!-- catalog-driven edge slot editors. -->
      <template v-if="kindMetadata">
        <section
          v-for="edgeSlot in kindMetadata.edge_slots"
          :key="edgeSlot.key"
          class="form-section"
        >
          <h3>{{ edgeSlot.label }}</h3>
          <p v-if="edgeSlot.description" class="hint">{{ edgeSlot.description }}</p>
          <CatalogEdgeSlotEditor
            :edge-slot="edgeSlot"
            :model-value="slotValue(edgeSlot)"
            :node-options="nodeIdOptions"
            :expression-context="expressionContext"
            @update:model-value="setSlotValue(edgeSlot, $event)"
          />
        </section>
      </template>
      <section
        v-else-if="!isProtectedNode"
        class="form-section catalog-loading-section"
      >
        <h3>Control Flow</h3>
        <p class="hint catalog-loading-inline">
          <LoadingSpinner size="sm" label="Loading node metadata" />
          Loading node metadata…
        </p>
      </section>

      <section class="form-section">
        <h3>Transitions</h3>
        <div class="transition-grid">
          <label v-for="key in workflows.directTransitionKeys" :key="key">
            {{ key }}
            <select
              :value="workflows.getTransition(key)"
              @change="workflows.setTransition(key, ($event.target as HTMLSelectElement).value)"
            >
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
        </div>
      </section>

      <section v-if="referenceGroups.length" class="form-section">
        <h3>Available References</h3>
        <ReferenceChips :groups="referenceGroups" />
      </section>

      <p v-if="workflows.stepEditorError" class="error">{{ workflows.stepEditorError }}</p>
      <div class="modal-actions">
        <button type="button" class="btn" @click="workflows.closeStepEditor">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="savingStep">
          <LoadingSpinner v-if="savingStep" size="sm" label="Saving step" />
          {{ savingStep ? "Applying…" : "Apply Step" }}
        </button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import { pretty } from "../../../core/utils/format";
import type { JsonRecord, NodeFieldMetadata, NodeEdgeSlot } from "../../../core/domain/models";
import { workflowInputType } from "../../../core/domain/models";
import { parseObject } from "../../../core/utils/json";
import AdvancedWdlParameters from "../shared/AdvancedWdlParameters.vue";
import KeyValueObjectEditor from "../shared/KeyValueObjectEditor.vue";
import ReferenceChips from "../shared/ReferenceChips.vue";
import { buildSampleContext, workflowReferenceGroups } from "../../../core/utils/workflow-references";
import { asArray, isRecord, recordArray, workflowNodeKindLabel, setAtLocation, getAtLocation } from "../../../core/workflow";
import { displayValue } from "../../../core/utils/values";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";
import CatalogEdgeSlotEditor from "./CatalogEdgeSlotEditor.vue";
import { findNodeKindMetadata, cloneTemplate } from "../../../core/workflow";
import { useOperationLoading } from "../../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const catalogMetadata = useCatalogMetadataStore();
const { isLoading: savingStep } = useOperationLoading("Saving workflow");

// kind metadata from the backend catalog.
const kindMetadata = computed(() => catalogMetadata.nodeKind(workflows.stepEditor.kind));

// node id list for selects, excluding the current step.
const targetNodes = computed(() => {
  const nodes = asArray(workflows.workflowDraft.definition.nodes).filter(isRecord);
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});

const nodeIdOptions = computed(() =>
  targetNodes.value.map((node) => displayValue(node.id)).filter(Boolean),
);

const availableSubflows = computed(() => {
  const currentId = workflows.selectedWorkflowId;
  return workflows.workflows.filter((w) => w.id !== currentId);
});

const isProtectedNode = computed(() =>
  ["start", "end", "fail"].includes(displayValue(workflows.selectedNode?.kind ?? "")),
);

// --- kind change ---

function onKindChange() {
  const kind = workflows.stepEditor.kind;
  const meta = findNodeKindMetadata(kind);

  if (!meta) {
    return;
  }

  const template = cloneTemplate(meta.default_template);
  // preserve id, name, kind and runtime fields; swap the rest from the catalog template.
  const { id, name, retry, locked, skipped, timeout_seconds } = workflows.stepEditor.nodeDraft as JsonRecord & {
    id?: string; name?: string; retry?: JsonRecord; locked?: boolean; skipped?: boolean; timeout_seconds?: number;
  };
  workflows.stepEditor.nodeDraft = {
    ...template,
    id: id ?? workflows.stepEditor.id,
    kind,
    ...(name ? { name } : {}),
    ...(retry ? { retry } : {}),
    ...(locked ? { locked } : {}),
    ...(skipped ? { skipped } : {}),
    ...(timeout_seconds ? { timeout_seconds } : {}),
  };
}

// --- field read/write via catalog field locations ---

function fieldValue(field: NodeFieldMetadata): unknown {
  const draft = workflows.stepEditor.nodeDraft;
  if (!draft || typeof draft !== "object") {return undefined;}
  return getAtLocation(draft, field.location);
}

function setFieldValue(field: NodeFieldMetadata, value: unknown) {
  const draft = workflows.stepEditor.nodeDraft;
  if (!draft || typeof draft !== "object") {return;}
  workflows.stepEditor.nodeDraft = setAtLocation(draft, field.location, value);
}

// --- edge slot read/write ---

function slotValue(slot: NodeEdgeSlot): unknown {
  const draft = workflows.stepEditor.nodeDraft;
  if (!draft || typeof draft !== "object") {return undefined;}
  return getAtLocation(draft, slot.target);
}

function setSlotValue(slot: NodeEdgeSlot, value: unknown) {
  const draft = workflows.stepEditor.nodeDraft;
  if (!draft || typeof draft !== "object") {return;}
  workflows.stepEditor.nodeDraft = setAtLocation(draft, slot.target, value);
}

// --- action-specific bindings ---

// sibling values for CatalogFieldEditor to resolve the active provider for action_function.
const actionSiblingValues = computed((): Record<string, unknown> => {
  const actionDraft = (workflows.stepEditor.nodeDraft)?.action;
  if (!actionDraft || typeof actionDraft !== "object" || Array.isArray(actionDraft)) {return {};}
  return { provider: (actionDraft as JsonRecord).provider };
});

const currentProvider = computed(
  () =>
    providersStore.providers.find(
      (provider) => {
        const actionDraft = (workflows.stepEditor.nodeDraft)?.action as JsonRecord | undefined;
        return provider.name === actionDraft?.provider;
      },
    ) ?? null,
);

const currentActions = computed(() => currentProvider.value?.actions ?? []);

const selectedAction = computed(
  () => {
    const actionDraft = (workflows.stepEditor.nodeDraft)?.action as JsonRecord | undefined;
    return currentActions.value.find(
      (action) => action.function_name === actionDraft?.function,
    ) ?? null;
  },
);

// action.configuration is bound directly into nodeDraft for TypedParameterEditor.
const actionConfiguration = computed({
  get: (): JsonRecord => {
    const actionDraft = (workflows.stepEditor.nodeDraft)?.action;
    if (!actionDraft || typeof actionDraft !== "object" || Array.isArray(actionDraft)) {return {};}
    return ((actionDraft as JsonRecord).configuration as JsonRecord | undefined) ?? {};
  },
  set: (value: JsonRecord) => {
    const draft = workflows.stepEditor.nodeDraft;
    const action = draft.action && typeof draft.action === "object" && !Array.isArray(draft.action)
      ? (draft.action as JsonRecord)
      : {};
    workflows.stepEditor.nodeDraft = { ...draft, action: { ...action, configuration: value } };
  },
});

// raw json string escape hatch for AdvancedWdlParameters.
const actionConfigurationJson = computed({
  get: () => pretty(actionConfiguration.value),
  set: (text: string) => {
    const parsed = parseObject(text, actionConfiguration.value);
    actionConfiguration.value = parsed;
  },
});

// --- expression context ---

const expressionContext = computed(() => ({
  workflowInputType: workflowInputType(workflows.workflowDraft),
  nodes: recordArray(workflows.workflowDraft.definition.nodes),
  currentNodeId: workflows.selectedStepId,
  providers: providersStore.providers,
  sampleContext: buildSampleContext(workflows.workflowRunDetail),
}));

const referenceGroups = computed(() => workflowReferenceGroups(expressionContext.value));

// --- focus modal on open ---

const modalRoot = ref<HTMLElement | null>(null);

onMounted(() => {
  if (providersStore.providers.length === 0 && !providersStore.loading) {
    void providersStore.fetchProviders();
  }

  modalRoot.value?.focus();
});
</script>

<style scoped>
.step-modal {
  width: min(1040px, 100%);
}

.modal-header span,
.result-metadata {
  color: var(--text-muted);
  font-size: 12px;
}

.hint {
  color: var(--text-muted);
  font-size: 12px;
}

.catalog-loading-inline {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.transition-grid {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(5, minmax(0, 1fr));
}

@media (max-width: 760px) {
  .transition-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
