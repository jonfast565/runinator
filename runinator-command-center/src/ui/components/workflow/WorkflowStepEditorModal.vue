<template>
  <div
    ref="modalRoot"
    class="modal-backdrop"
    tabindex="-1"
    @keydown.esc.stop.prevent="workflows.closeStepEditor"
  >
    <form class="modal step-editor-modal" @submit.prevent="workflows.submitStepEditor">
      <header class="modal-header">
        <div class="step-editor-title">
          <div class="flex items-center gap-1">
            <h2>{{ workflows.stepEditorCreating ? "Add Workflow Step" : "Edit Workflow Step" }}</h2>
            <HelpBubble
              text="Configure the step's identity, runtime behavior, parameters, and outgoing control flow."
              label="About the workflow step editor"
            />
          </div>
          <p>{{ workflows.selectedStepId || "New step" }}</p>
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

      <div class="step-editor-body">
        <section class="form-section step-editor-section">
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Step</p>
              <h3>Identity &amp; behavior</h3>
              <p class="hint">
                Give this node a stable ID, then choose the behavior it adds to the workflow.
              </p>
            </div>
          </div>
          <div class="step-editor-identity-grid">
            <label :class="{ 'has-error': stepValidation.errors.id }">
              <span>Step ID</span>
              <input
                v-model="workflows.stepEditor.id"
                type="text"
                required
                autocomplete="off"
                :aria-invalid="Boolean(stepValidation.errors.id)"
                aria-describedby="step-id-help step-id-error"
              />
              <small id="step-id-help"
                >Used by transitions and expressions; it must be unique within this workflow.</small
              >
              <small
                v-if="stepValidation.errors.id"
                id="step-id-error"
                class="field-error"
                role="alert"
              >
                {{ stepValidation.errors.id }}
              </small>
            </label>
            <label>
              <span>Display name</span>
              <input
                v-model="workflows.stepEditor.name"
                placeholder="Shown on the node; defaults to the step ID"
              />
              <small>Optional. This is the label readers see on the workflow canvas.</small>
            </label>
            <label :class="{ 'has-error': stepValidation.errors.kind }">
              <span>Node kind</span>
              <select
                v-model="workflows.stepEditor.kind"
                :disabled="workflows.selectedStepKindLocked"
                :aria-invalid="Boolean(stepValidation.errors.kind)"
                @change="onKindChange"
              >
                <!-- the non-addable kinds are listed so an existing node of one displays its own
                     kind; the select is disabled for all of them, so none can be chosen. -->
                <option value="start">start</option>
                <option value="interrupt">interrupt</option>
                <option v-for="kind in editableNodeKinds" :key="kind" :value="kind">
                  {{ workflowNodeKindLabel(kind) }}
                </option>
                <option value="end">end</option>
                <option value="fail">fail</option>
              </select>
              <small v-if="stepValidation.errors.kind" class="field-error" role="alert">
                {{ stepValidation.errors.kind }}
              </small>
              <small v-else-if="isHandlerRegionStep" class="text-fg-muted">
                Only node types that can safely run inside an interrupt handler are shown.
              </small>
              <small v-else
                >Changing the kind preserves this step’s identity and retry policy.</small
              >
            </label>
          </div>
        </section>

        <section class="form-section step-editor-section">
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Execution</p>
              <h3>Runtime controls</h3>
            </div>
          </div>
          <div class="step-editor-runtime-grid">
            <div class="step-editor-toggle-card">
              <label class="checkbox">
                <input
                  v-model="workflows.stepEditor.locked"
                  type="checkbox"
                  :disabled="isProtectedNode"
                />
                Locked
              </label>
              <p>Prevent structural edits to this step.</p>
            </div>
            <div class="step-editor-toggle-card">
              <label class="checkbox">
                <input v-model="workflows.stepEditor.skipped" type="checkbox" />
                Skipped
              </label>
              <p>Bypass this step when the workflow runs.</p>
            </div>
            <label :class="{ 'has-error': stepValidation.errors.timeout }">
              <span>Node timeout (seconds)</span>
              <input
                v-model.number="workflows.stepEditor.timeout_seconds"
                type="number"
                min="0"
                step="1"
                :aria-invalid="Boolean(stepValidation.errors.timeout)"
                aria-describedby="step-timeout-help step-timeout-error"
              />
              <small id="step-timeout-help">Use 0 to leave this node without a deadline.</small>
              <small
                v-if="stepValidation.errors.timeout"
                id="step-timeout-error"
                class="field-error"
                role="alert"
              >
                {{ stepValidation.errors.timeout }}
              </small>
            </label>
          </div>
        </section>

        <section class="form-section step-editor-section">
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Recovery</p>
              <h3>Retry policy</h3>
            </div>
          </div>
          <RetryPolicyEditor v-model="retryPolicy" />
          <p
            v-if="stepValidation.errors.retry"
            class="field-error step-editor-inline-error"
            role="alert"
          >
            {{ stepValidation.errors.retry }}
          </p>
        </section>

        <!-- compensation is an action-node property end to end: only `lower_action` reads the
             `compensate` clause, and only an action node's decompile writes one back. -->
        <section
          v-if="workflows.stepEditor.kind === 'action'"
          class="form-section step-editor-section"
        >
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Optional recovery</p>
              <h3>Compensation</h3>
              <p class="hint">Run another action if this step cannot complete successfully.</p>
            </div>
          </div>
          <CompensationEditor v-model="compensation" :expression-context="expressionContext" />
        </section>

        <!-- catalog-driven parameter fields for this node kind. -->
        <section
          v-if="kindMetadata && kindMetadata.fields.length"
          class="form-section step-editor-section"
        >
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Configuration</p>
              <h3>Parameters</h3>
              <p class="hint">
                Fields marked with an asterisk are required before this step can be applied.
              </p>
            </div>
          </div>
          <div class="step-editor-field-list">
            <div
              v-for="field in kindMetadata.fields"
              :key="field.name"
              class="step-editor-catalog-field"
              :class="{ 'has-error': stepValidation.errors.fields[field.name] }"
            >
              <CatalogFieldEditor
                :field="field"
                :model-value="fieldValue(field)"
                :expression-context="expressionContext"
                :node-options="nodeIdOptions"
                :workflows="availableSubflows"
                :sibling-values="actionSiblingValues"
                @update:model-value="setFieldValue(field, $event)"
              />
              <small
                v-if="stepValidation.errors.fields[field.name]"
                class="field-error"
                role="alert"
              >
                {{ stepValidation.errors.fields[field.name] }}
              </small>
            </div>
          </div>
        </section>
        <section
          v-else-if="!kindMetadata && !isProtectedNode"
          class="form-section step-editor-section catalog-loading-section"
        >
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Configuration</p>
              <h3>Parameters</h3>
            </div>
          </div>
          <p class="hint catalog-loading-hint">
            <LoadingSpinner size="sm" label="Loading node metadata" />
            Loading node metadata…
          </p>
        </section>

        <!-- action configuration via TypedParameterEditor when a provider action is selected. -->
        <section
          v-if="workflows.stepEditor.kind === 'action' && selectedAction"
          class="form-section step-editor-section"
        >
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Action detail</p>
              <h3>Action parameters</h3>
              <p class="hint">Inputs for {{ selectedAction.function_name }}.</p>
            </div>
          </div>
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
          <AdvancedRexRapParameters
            v-model="actionConfigurationJson"
            :context="expressionContext"
            title="Raw REXRAP parameters"
          />
        </section>

        <!-- catalog-driven edge slot editors. -->
        <template v-if="kindMetadata">
          <section
            v-for="edgeSlot in kindMetadata.edge_slots"
            :key="edgeSlot.key"
            class="form-section step-editor-section"
          >
            <div class="step-editor-section-heading">
              <div>
                <p class="step-editor-eyebrow">Control flow</p>
                <div class="flex items-center gap-1">
                  <h3>{{ edgeSlot.label }}</h3>
                  <HelpBubble
                    v-if="edgeSlot.description"
                    :text="edgeSlot.description"
                    :label="`About ${edgeSlot.label}`"
                  />
                </div>
              </div>
            </div>
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
          class="form-section step-editor-section catalog-loading-section"
        >
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Control flow</p>
              <h3>Connections</h3>
            </div>
          </div>
          <p class="hint catalog-loading-hint">
            <LoadingSpinner size="sm" label="Loading node metadata" />
            Loading node metadata…
          </p>
        </section>

        <section class="form-section step-editor-section">
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Control flow</p>
              <h3>Transitions</h3>
              <p class="hint">Choose where each direct outcome should continue.</p>
            </div>
          </div>
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

        <section v-if="referenceGroups.length" class="form-section step-editor-section">
          <div class="step-editor-section-heading">
            <div>
              <p class="step-editor-eyebrow">Authoring aid</p>
              <h3>Available references</h3>
            </div>
          </div>
          <ReferenceChips :groups="referenceGroups" />
        </section>
      </div>

      <p v-if="stepValidation.error" class="step-editor-validation-summary" role="alert">
        Fix the highlighted fields before applying this step.
      </p>
      <p v-if="workflows.stepEditorError" class="error">{{ workflows.stepEditorError }}</p>
      <div class="modal-actions step-editor-actions">
        <button type="button" class="btn" @click="workflows.closeStepEditor">Cancel</button>
        <button
          type="submit"
          class="btn btn-primary"
          :disabled="savingStep || Boolean(stepValidation.error)"
        >
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
import AdvancedRexRapParameters from "../shared/AdvancedRexRapParameters.vue";
import KeyValueObjectEditor from "../shared/KeyValueObjectEditor.vue";
import ReferenceChips from "../shared/ReferenceChips.vue";
import {
  buildSampleContext,
  workflowReferenceGroups,
} from "../../../core/utils/workflow-references";
import { workflowNodeKindLabel, setAtLocation, getAtLocation } from "../../../core/workflow";
import { jsonRecordArray as recordArray } from "../../../core/domain/json";
import { displayValue } from "../../../core/utils/values";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";
import Icon from "../shared/Icon.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";
import CatalogEdgeSlotEditor from "./CatalogEdgeSlotEditor.vue";
import CompensationEditor from "./CompensationEditor.vue";
import RetryPolicyEditor from "./RetryPolicyEditor.vue";
import type { RetryPolicy } from "../../../core/workflow/retry";
import { findNodeKindMetadata, cloneTemplate } from "../../../core/workflow";
import { interruptRegionOrigins } from "../../../core/workflow/interrupt-regions";
import { validateStepEditor } from "../../../core/workflow/step-editor-validation";
import { useOperationLoading } from "../../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const catalogMetadata = useCatalogMetadataStore();
const { isLoading: savingStep } = useOperationLoading("Saving workflow");

// kind metadata from the backend catalog.
const kindMetadata = computed(() => catalogMetadata.nodeKind(workflows.stepEditor.kind));

// node id list for selects, excluding the current step.
const targetNodes = computed(() => {
  const nodes = recordArray(workflows.workflowDraft.definition.nodes);
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});

const stepValidation = computed(() =>
  validateStepEditor(
    workflows.stepEditor,
    workflows.selectedStepId,
    recordArray(workflows.workflowDraft.definition.nodes),
    kindMetadata.value,
  ),
);

const nodeIdOptions = computed(() =>
  targetNodes.value.map((node) => displayValue(node.id)).filter(Boolean),
);

const availableSubflows = computed(() => {
  const currentId = workflows.selectedWorkflowId;
  return workflows.workflows.filter((w) => w.id !== currentId);
});

const isProtectedNode = computed(() =>
  ["start", "interrupt", "end", "fail"].includes(displayValue(workflows.selectedNode?.kind ?? "")),
);

const isHandlerRegionStep = computed(() =>
  interruptRegionOrigins(workflows.workflowDraft).has(workflows.selectedStepId),
);

const editableNodeKinds = computed(() => {
  if (!isHandlerRegionStep.value) {
    return workflows.workflowNodeKinds;
  }

  return workflows.workflowNodeKinds.filter(
    (kind) => findNodeKindMetadata(kind)?.handler_safe === true,
  );
});

// --- retry policy ---

// the retry fields live on the step editor state rather than in `nodeDraft`, because applying a
// step rebuilds the node's `retry` object from them.
const retryPolicy = computed<RetryPolicy>({
  get: () => ({
    max_attempts: workflows.stepEditor.max_attempts,
    backoff_base_seconds: workflows.stepEditor.backoff_base_seconds,
    backoff_max_seconds: workflows.stepEditor.backoff_max_seconds,
    jitter: workflows.stepEditor.jitter,
    retry_on: workflows.stepEditor.retry_on,
  }),
  set: (value) => {
    workflows.stepEditor.max_attempts = value.max_attempts;
    workflows.stepEditor.backoff_base_seconds = value.backoff_base_seconds;
    workflows.stepEditor.backoff_max_seconds = value.backoff_max_seconds;
    workflows.stepEditor.jitter = value.jitter;
    workflows.stepEditor.retry_on = value.retry_on;
  },
});

// --- compensation ---

const compensation = computed<JsonRecord | null>({
  get: () => {
    const value = workflows.stepEditor.nodeDraft.compensation;
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as JsonRecord)
      : null;
  },
  set: (value) => {
    const draft: JsonRecord = { ...workflows.stepEditor.nodeDraft };

    if (value === null) {
      // drop the key rather than nulling it: the node model skips serializing an absent
      // compensation, so a lingering `"compensation": null` would show up as noise in the json and
      // rexrap panes for a step that has none.
      Reflect.deleteProperty(draft, "compensation");
    } else {
      draft.compensation = value;
    }

    workflows.stepEditor.nodeDraft = draft;
  },
});

// --- kind change ---

function onKindChange() {
  const kind = workflows.stepEditor.kind;
  const meta = findNodeKindMetadata(kind);

  if (!meta) {
    return;
  }

  const template = cloneTemplate(meta.default_template);
  // preserve id, name, kind and runtime fields; swap the rest from the catalog template.
  const { id, name, retry, transitions, locked, skipped, timeout_seconds } = workflows.stepEditor
    .nodeDraft as JsonRecord & {
    id?: string;
    name?: string;
    retry?: JsonRecord;
    transitions?: JsonRecord;
    locked?: boolean;
    skipped?: boolean;
    timeout_seconds?: number;
  };
  workflows.stepEditor.nodeDraft = {
    ...template,
    id: id ?? workflows.stepEditor.id,
    kind,
    ...(name ? { name } : {}),
    ...(retry ? { retry } : {}),
    // changing the body step's kind must not quietly reconnect the handler to a template target
    // such as the main flow's `end`; its existing continuation is part of the region structure.
    ...(isHandlerRegionStep.value && transitions ? { transitions } : {}),
    ...(locked ? { locked } : {}),
    ...(skipped ? { skipped } : {}),
    ...(timeout_seconds ? { timeout_seconds } : {}),
  };
}

// --- field read/write via catalog field locations ---

function fieldValue(field: NodeFieldMetadata): unknown {
  const draft = workflows.stepEditor.nodeDraft;
  return getAtLocation(draft, field.location);
}

function setFieldValue(field: NodeFieldMetadata, value: unknown) {
  const draft = workflows.stepEditor.nodeDraft;
  workflows.stepEditor.nodeDraft = setAtLocation(draft, field.location, value);
}

// --- edge slot read/write ---

function slotValue(slot: NodeEdgeSlot): unknown {
  const draft = workflows.stepEditor.nodeDraft;
  return getAtLocation(draft, slot.target);
}

function setSlotValue(slot: NodeEdgeSlot, value: unknown) {
  const draft = workflows.stepEditor.nodeDraft;
  workflows.stepEditor.nodeDraft = setAtLocation(draft, slot.target, value);
}

// --- action-specific bindings ---

// sibling values for CatalogFieldEditor to resolve the active provider for action_function.
const actionSiblingValues = computed((): Record<string, unknown> => {
  const actionDraft = workflows.stepEditor.nodeDraft.action;

  if (!actionDraft || typeof actionDraft !== "object" || Array.isArray(actionDraft)) {
    return {};
  }

  return { provider: (actionDraft as JsonRecord).provider };
});

const currentProvider = computed(
  () =>
    providersStore.providers.find((provider) => {
      const actionDraft = workflows.stepEditor.nodeDraft.action as JsonRecord | undefined;
      return provider.name === actionDraft?.provider;
    }) ?? null,
);

const currentActions = computed(() => currentProvider.value?.actions ?? []);

const selectedAction = computed(() => {
  const actionDraft = workflows.stepEditor.nodeDraft.action as JsonRecord | undefined;
  return (
    currentActions.value.find((action) => action.function_name === actionDraft?.function) ?? null
  );
});

// action.configuration is bound directly into nodeDraft for TypedParameterEditor.
const actionConfiguration = computed({
  get: (): JsonRecord => {
    const actionDraft = workflows.stepEditor.nodeDraft.action;

    if (!actionDraft || typeof actionDraft !== "object" || Array.isArray(actionDraft)) {
      return {};
    }

    return ((actionDraft as JsonRecord).configuration as JsonRecord | undefined) ?? {};
  },
  set: (value: JsonRecord) => {
    const draft = workflows.stepEditor.nodeDraft;
    const action =
      draft.action && typeof draft.action === "object" && !Array.isArray(draft.action)
        ? (draft.action as JsonRecord)
        : {};
    workflows.stepEditor.nodeDraft = { ...draft, action: { ...action, configuration: value } };
  },
});

// raw json string escape hatch for AdvancedRexRapParameters.
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
