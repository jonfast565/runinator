<template>
  <div class="modal-backdrop" @click.self="workflows.closeStepEditor">
    <form class="modal step-modal" @submit.prevent="workflows.submitStepEditor">
      <header class="modal-header">
        <div>
          <h2>{{ workflows.stepEditorCreating ? "Add Workflow Step" : "Edit Workflow Step" }}</h2>
          <span>{{ workflows.selectedStepId || "New step" }}</span>
        </div>
        <div class="modal-header-actions">
          <button type="submit" class="primary">Apply Step</button>
          <button type="button" @click="workflows.closeStepEditor">Close</button>
        </div>
      </header>

      <section class="form-section">
        <h3>Step</h3>
        <div class="form-grid">
          <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
          <label>Name <input v-model="workflows.stepEditor.name" placeholder="Shown on the node; defaults to the step ID" /></label>
          <label>
            Node Kind
            <select v-model="workflows.stepEditor.kind" :disabled="workflows.selectedStepKindLocked">
              <option value="start">start</option>
              <option v-for="kind in workflows.workflowNodeKinds" :key="kind" :value="kind">{{ kind }}</option>
              <option value="end">end</option>
              <option value="fail">fail</option>
            </select>
          </label>
          <label class="checkbox">
            <input v-model="workflows.stepEditor.locked" type="checkbox" :disabled="isProtectedNode" />
            Locked
          </label>
          <label class="checkbox">
            <input v-model="workflows.stepEditor.skipped" type="checkbox" />
            Skipped
          </label>
          <label>Max Attempts <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1" /></label>
          <label>Timeout Seconds <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0" /></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'action'" class="form-section">
        <div class="section-title-row">
          <h3>Action Configuration</h3>
        </div>
        <div class="form-grid">
          <label>
            Action Name
            <select :value="workflows.stepEditor.action_name" @change="onActionNameChange">
              <option value="" disabled>Select action name</option>
              <option v-if="selectedProviderMissing" :value="workflows.stepEditor.action_name">{{ workflows.stepEditor.action_name }} (unavailable)</option>
              <option v-for="provider in providersStore.providers" :key="provider.name" :value="provider.name">{{ provider.name }}</option>
            </select>
          </label>
          <label>
            Action Function
            <select v-model="workflows.stepEditor.action_function" :disabled="!currentProvider" @change="applyParameterDefaults">
              <option value="" disabled>{{ currentProvider ? "Select action function" : "Select action name first" }}</option>
              <option v-if="selectedActionMissing" :value="workflows.stepEditor.action_function">{{ workflows.stepEditor.action_function }} (unavailable)</option>
              <option v-for="action in currentActions" :key="action.function_name" :value="action.function_name">{{ action.function_name }}</option>
            </select>
          </label>
        </div>
        <p v-if="selectedAction?.results?.length" class="result-metadata">
          Results:
          <span v-for="result in selectedAction.results" :key="result.name">{{ result.name }} ({{ result.ty?.type ?? "any" }})</span>
        </p>
      </section>

      <section v-if="workflows.stepEditor.kind === 'action'" class="form-section">
        <h3>Step Parameters</h3>
        <TypedParameterEditor
          v-if="selectedAction"
          v-model="stepParameters"
          :parameters="selectedAction.parameters ?? []"
          :credential-scopes="currentProvider?.metadata.credential_scopes ?? []"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'approval'" class="form-section">
        <h3>Approval</h3>
        <div class="form-grid">
          <label>Approval Type <input v-model="workflows.stepEditor.approval_type" /></label>
          <label>Prompt <textarea v-model="workflows.stepEditor.approval_prompt"></textarea></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'gate'" class="form-section">
        <h3>Gate</h3>
        <div class="form-grid">
          <label>
            Kind
            <select v-model="workflows.stepEditor.gate_kind">
              <option value="manual">manual</option>
              <option value="condition">condition</option>
              <option value="external">external</option>
            </select>
          </label>
          <label>Label <input v-model="workflows.stepEditor.gate_label" placeholder="Shown in the Gates view" /></label>
          <label>Poll Interval (seconds) <input v-model.number="workflows.stepEditor.gate_poll_interval" type="number" min="1" /></label>
          <label>Timeout (seconds, 0 = none) <input v-model.number="workflows.stepEditor.gate_timeout" type="number" min="0" /></label>
        </div>
        <p class="hint">{{ gateKindHint }}</p>
        <div v-if="workflows.stepEditor.gate_kind === 'condition'" class="form-field">
          <span class="form-field-label">When (passes once true)</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.gate_when_json" :context="expressionContext" title="Gate condition" />
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'signal'" class="form-section">
        <h3>Signal</h3>
        <div class="form-grid">
          <label>Signal Name <input v-model="workflows.stepEditor.signal_name" placeholder="Name delivered to POST /workflow_runs/{id}/signals" /></label>
        </div>
        <p class="hint">Pauses the run until this named signal is delivered. Set a node timeout to bound the wait.</p>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'condition'" class="form-section">
        <h3>Condition Branches</h3>
        <div v-for="(branch, index) in workflows.stepEditor.condition_branches" :key="index" class="condition-branch-row">
          <div class="form-field">
            <span class="form-field-label">When</span>
            <ExpressionJsonEditor v-model="branch.when_json" :context="expressionContext" title="Condition branch" />
          </div>
          <label>
            Target
            <select v-model="branch.target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <button type="button" @click="workflows.removeConditionBranchEditor(index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addConditionBranchEditor">Add Branch</button>
        <label>
          Fallback
          <select v-model="workflows.stepEditor.condition_fallback">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
          </select>
        </label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'wait'" class="form-section">
        <h3>Wait</h3>
        <div class="form-grid">
          <label>Seconds <input v-model.number="workflows.stepEditor.wait_seconds" type="number" min="0" /></label>
          <label>Initial Status <input v-model="workflows.stepEditor.wait_initial_status" /></label>
          <label>Until Status <input v-model="workflows.stepEditor.wait_until_status" /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Advanced Wait Settings</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.wait_json" :context="expressionContext" title="Wait settings" />
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'loop'" class="form-section">
        <h3>Loop</h3>
        <div class="form-grid">
          <label>
            Target
            <select v-model="workflows.stepEditor.loop_target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <label>Max Iterations <input v-model.number="workflows.stepEditor.loop_max_iterations" type="number" min="1" /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Items</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.loop_items_json" :context="expressionContext" title="Loop items" />
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'switch'" class="form-section">
        <h3>Switch</h3>
        <div class="form-field">
          <span class="form-field-label">Value</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.switch_value_json" :context="expressionContext" title="Switch value" />
        </div>
        <div v-for="(switchCase, index) in workflows.stepEditor.switch_cases" :key="index" class="condition-branch-row">
          <label>
            Match
            <select v-model="switchCase.match_kind">
              <option value="equals">equals</option>
              <option value="not_equals">not_equals</option>
              <option value="exists">exists</option>
              <option value="when">when</option>
            </select>
          </label>
          <div class="form-field">
            <span class="form-field-label">Value</span>
            <ExpressionJsonEditor v-model="switchCase.match_json" :context="expressionContext" title="Switch case match" />
          </div>
          <label>
            Target
            <select v-model="switchCase.target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <button type="button" @click="workflows.removeSwitchCaseEditor(index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addSwitchCaseEditor">Add Case</button>
        <label>
          Default
          <select v-model="workflows.stepEditor.switch_default">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
          </select>
        </label>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'parallel'" class="form-section">
        <h3>Parallel</h3>
        <div v-for="(_, index) in workflows.stepEditor.parallel_branches" :key="index" class="condition-branch-row">
          <label>
            Branch
            <select v-model="workflows.stepEditor.parallel_branches[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <button type="button" @click="workflows.removeNodeRefEditor(workflows.stepEditor.parallel_branches, index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addNodeRefEditor(workflows.stepEditor.parallel_branches)">Add Branch</button>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'join'" class="form-section">
        <h3>Join</h3>
        <label>
          Mode
          <select v-model="workflows.stepEditor.join_mode">
            <option v-for="policy in branchPolicies" :key="policy" :value="policy">{{ policy }}</option>
          </select>
        </label>
        <div v-for="(_, index) in workflows.stepEditor.join_wait_for" :key="index" class="condition-branch-row">
          <label>
            Wait For
            <select v-model="workflows.stepEditor.join_wait_for[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <button type="button" @click="workflows.removeNodeRefEditor(workflows.stepEditor.join_wait_for, index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addNodeRefEditor(workflows.stepEditor.join_wait_for)">Add Dependency</button>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'try'" class="form-section">
        <h3>Try</h3>
        <div class="form-grid">
          <label>
            Body
            <select v-model="workflows.stepEditor.try_body">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <label>
            Catch
            <select v-model="workflows.stepEditor.try_catch">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <label>
            Finally
            <select v-model="workflows.stepEditor.try_finally">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'map'" class="form-section">
        <h3>Map</h3>
        <div class="form-grid">
          <label>
            Target
            <select v-model="workflows.stepEditor.map_target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <label>Concurrency <input v-model.number="workflows.stepEditor.map_concurrency" type="number" min="1" /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Items</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.map_items_json" :context="expressionContext" title="Map items" />
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'race'" class="form-section">
        <h3>Race</h3>
        <label>
          Winner
          <select v-model="workflows.stepEditor.race_winner">
            <option v-for="policy in branchPolicies" :key="policy" :value="policy">{{ policy }}</option>
          </select>
        </label>
        <div v-for="(_, index) in workflows.stepEditor.race_branches" :key="index" class="condition-branch-row">
          <label>
            Branch
            <select v-model="workflows.stepEditor.race_branches[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
            </select>
          </label>
          <button type="button" @click="workflows.removeNodeRefEditor(workflows.stepEditor.race_branches, index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addNodeRefEditor(workflows.stepEditor.race_branches)">Add Branch</button>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'output'" class="form-section">
        <h3>Output</h3>
        <label>Event Type <input v-model="workflows.stepEditor.output_event_type" /></label>
        <div class="form-field">
          <span class="form-field-label">Data</span>
          <TypedValueEditor
            :ty="outputDataType"
            :model-value="outputData"
            :expression-context="expressionContext"
            @update:model-value="onOutputDataChange"
          />
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.output_data_json" :context="expressionContext" title="Advanced WDL data" />
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'input'" class="form-section">
        <h3>Input</h3>
        <label>Prompt <input v-model="workflows.stepEditor.input_prompt" /></label>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'config'" class="form-section">
        <h3>Config</h3>
        <div class="form-field">
          <span class="form-field-label">Name</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.config_name_json" :context="expressionContext" title="Config name" />
        </div>
        <div class="form-field">
          <span class="form-field-label">Metadata</span>
          <ExpressionJsonEditor v-model="workflows.stepEditor.config_metadata_json" :context="expressionContext" title="Config metadata" />
        </div>
        <AdvancedWdlParameters v-model="workflows.stepEditor.parameters_json" :context="expressionContext" />
      </section>

      <section v-if="workflows.stepEditor.kind === 'subflow'" class="form-section">
        <h3>Subflow</h3>
        <div class="form-grid">
          <label>
            Workflow
            <select :value="selectedSubflowName || ''" @change="onSubflowNameChange">
              <option value="">Select a workflow</option>
             <option v-if="selectedSubflowMissing" :value="String(workflows.stepEditor.subflow_id ?? '')">{{ selectedSubflowName }} (unavailable)</option>
             <option v-for="workflow in availableSubflows" :key="String(workflow.id)" :value="workflow.name">{{ workflow.name }}</option>
            </select>
          </label>
        </div>
        <h3>Parameters</h3>
        <TypedValueEditor
          v-if="selectedSubflowInputType"
          :ty="selectedSubflowInputType"
          :model-value="subflowParameters"
          :expression-context="expressionContext"
          @update:model-value="onSubflowParametersChange"
        />
        <p v-else class="hint">Select a workflow to configure its parameters, or use the advanced editor below.</p>
        <AdvancedWdlParameters v-model="workflows.stepEditor.subflow_parameters_json" :context="expressionContext" />
      </section>

      <section class="form-section">
        <h3>Transitions</h3>
        <div class="transition-grid">
          <label v-for="key in workflows.directTransitionKeys" :key="key">
            {{ key }}
            <select :value="workflows.getTransition(key)" @change="workflows.setTransition(key, ($event.target as HTMLSelectElement).value)">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
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
        <button type="button" @click="workflows.closeStepEditor">Cancel</button>
        <button type="submit">Apply Step</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import { useProvidersStore } from "../../stores/providers";
import { buildInputSkeleton, useWorkflowsStore } from "../../stores/workflows";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import AdvancedWdlParameters from "../shared/AdvancedWdlParameters.vue";
import ReferenceChips from "../shared/ReferenceChips.vue";
import { buildSampleContext, workflowReferenceGroups } from "../../utils/workflow-references";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";
import TypedValueEditor from "../shared/TypedValueEditor.vue";
import type { RuninatorType } from "../../types/models";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const branchPolicies = ["all", "any", "first_success"] as const;

const currentProvider = computed(() => providersStore.providers.find(provider => provider.name === workflows.stepEditor.action_name) || null);
const currentActions = computed(() => currentProvider.value?.actions ?? []);
const selectedAction = computed(() => currentActions.value.find(action => action.function_name === workflows.stepEditor.action_function) ?? null);
const selectedProviderMissing = computed(() => Boolean(workflows.stepEditor.action_name && !currentProvider.value));
const selectedActionMissing = computed(() =>
  Boolean(workflows.stepEditor.action_function && currentProvider.value && !currentActions.value.some(action => action.function_name === workflows.stepEditor.action_function))
);
const stepParameters = computed({
  get: () => parseObject(workflows.stepEditor.parameters_json, {}),
  set: (value) => {
    workflows.stepEditor.parameters_json = pretty(value);
  }
});
const isProtectedNode = computed(() => ["start", "end", "fail"].includes(workflows.selectedNode?.kind ?? ""));
const gateKindHint = computed(() => {
  switch (workflows.stepEditor.gate_kind) {
    case "condition":
      return "The reducer auto-evaluates the condition each poll; the gate passes once it is true.";
    case "external":
      return "Stays closed until an external system opens it via POST /gates/{id}/open.";
    default:
      return "Stays closed until a human opens it from the Gates view.";
  }
});
const targetNodes = computed(() => {
  const nodes: any[] = workflows.workflowDraft.definition?.nodes ?? [];
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});
const expressionContext = computed(() => ({
  workflowInputType: workflows.workflowDraft.input_type ?? null,
  nodes: workflows.workflowDraft.definition?.nodes ?? [],
  currentNodeId: workflows.selectedStepId,
  providers: providersStore.providers,
  // a loaded run's data lets the editor preview resolved values against real outputs.
  sampleContext: buildSampleContext(workflows.workflowRunDetail)
}));

// the references in scope at this node (params, prior node outputs, run roots) for the chip list.
const referenceGroups = computed(() => workflowReferenceGroups(expressionContext.value));

const availableSubflows = computed(() => {
  const currentId = workflows.selectedWorkflowId;
  return workflows.workflows.filter((w) => w.id !== currentId);
});

const selectedSubflowName = computed(() => {
  if (!workflows.stepEditor.subflow_id) return "";
  const workflow = workflows.workflows.find((w) => w.id === workflows.stepEditor.subflow_id);
  return workflow?.name ?? "";
});

const selectedSubflowMissing = computed(() => {
  return Boolean(workflows.stepEditor.subflow_id && !selectedSubflowName.value);
});

// the child workflow's declared input schema drives the typed parameter form.
const selectedSubflowInputType = computed(() => {
  const workflow = workflows.workflows.find((w) => w.id === workflows.stepEditor.subflow_id);
  return workflow?.input_type ?? null;
});

const subflowParameters = computed(() => parseObject(workflows.stepEditor.subflow_parameters_json, {}));

// output data is an open "bag": no fixed fields, any number of named entries each holding any wdl value.
const outputDataType: RuninatorType = { type: "struct", fields: {}, additional: { type: "any" } };
const outputData = computed(() => parseObject(workflows.stepEditor.output_data_json, {}));

// the bag editor and the raw-json fallback both write back to the same json string.
function onOutputDataChange(value: unknown) {
  const object = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  workflows.stepEditor.output_data_json = pretty(object);
}

// the typed editor and the raw-json fallback both write back to the same json string.
function onSubflowParametersChange(value: unknown) {
  const object = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  workflows.stepEditor.subflow_parameters_json = pretty(object);
}

onMounted(() => {
  if (providersStore.providers.length === 0 && !providersStore.loading) providersStore.fetchProviders();
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
});

// escape closes the node editor without applying changes.
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") workflows.closeStepEditor();
}

function onActionNameChange(event: Event) {
  const name = (event.target as HTMLSelectElement).value;
  workflows.stepEditor.action_name = name;
  const provider = providersStore.providers.find(item => item.name === name);
  workflows.stepEditor.action_function = provider?.actions[0]?.function_name ?? "";
  applyParameterDefaults();
}

function applyParameterDefaults() {
  if (!selectedAction.value) return;
  const next = { ...stepParameters.value };
  for (const parameter of selectedAction.value.parameters ?? []) {
    if (next[parameter.name] === undefined && parameter.default_value !== undefined) next[parameter.name] = parameter.default_value;
  }
  stepParameters.value = next;
}

function onSubflowNameChange(event: Event) {
  const name = (event.target as HTMLSelectElement).value;
  const workflow = workflows.workflows.find(w => w.name === name);
  if (!workflow?.id) return;
  workflows.stepEditor.subflow_id = workflow.id;
  // seed declared fields when no parameters are set yet, so the form renders pre-populated.
  if (Object.keys(subflowParameters.value).length === 0) {
    onSubflowParametersChange(buildInputSkeleton(workflow.input_type ?? null));
  }
}
</script>

<style scoped>
.step-modal {
  width: min(1040px, 100%);
}

.modal-header span,
.result-metadata {
  color: #66717e;
  font-size: 12px;
}

.modal-header-actions {
  display: flex;
  gap: 8px;
  align-items: start;
}

.modal-header-actions .primary {
  background: #17202a;
  color: #ffffff;
}

.hint {
  color: #66717e;
  font-size: 12px;
}

.section-title-row {
  display: flex;
  align-items: end;
  justify-content: space-between;
  gap: 12px;
}

.section-title-row label {
  min-width: 260px;
}

.transition-grid {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(5, minmax(0, 1fr));
}

.condition-branch-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 180px auto;
  gap: 8px;
  align-items: end;
}

.result-metadata {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
</style>
