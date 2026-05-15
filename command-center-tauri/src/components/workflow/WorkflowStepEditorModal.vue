<template>
  <div class="modal-backdrop">
    <form class="modal step-modal" @submit.prevent="workflows.submitStepEditor">
      <header class="modal-header">
        <div>
          <h2>{{ workflows.stepEditorCreating ? "Add Workflow Step" : "Edit Workflow Step" }}</h2>
          <span>{{ workflows.selectedStepId || "New step" }}</span>
        </div>
        <button type="button" @click="workflows.closeStepEditor">Close</button>
      </header>

      <section class="form-section">
        <h3>Step</h3>
        <div class="form-grid">
          <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
          <label>
            Node Kind
            <select v-model="workflows.stepEditor.kind" :disabled="isProtectedNode">
              <option value="start">start</option>
              <option v-for="kind in workflows.workflowNodeKinds" :key="kind" :value="kind">{{ kind }}</option>
              <option value="end">end</option>
              <option value="fail">fail</option>
            </select>
          </label>
          <label>Max Attempts <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1" /></label>
          <label>Timeout Seconds <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0" /></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'task' || workflows.stepEditor.kind === 'action'" class="form-section">
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
          <span v-for="result in selectedAction.results" :key="result.name">{{ result.name }} ({{ result.value_type }})</span>
        </p>
      </section>

      <section v-if="workflows.stepEditor.kind === 'task' || workflows.stepEditor.kind === 'action'" class="form-section">
        <h3>Step Parameters</h3>
        <TypedParameterEditor
          v-if="selectedAction"
          v-model="stepParameters"
          :parameters="selectedAction.parameters ?? []"
          :credential-scopes="currentProvider?.metadata.credential_scopes ?? []"
        />
        <label>Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'approval'" class="form-section">
        <h3>Approval</h3>
        <div class="form-grid">
          <label>Approval Type <input v-model="workflows.stepEditor.approval_type" /></label>
          <label>Prompt <textarea v-model="workflows.stepEditor.approval_prompt"></textarea></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'condition'" class="form-section">
        <h3>Condition Branches</h3>
        <div v-for="(branch, index) in workflows.stepEditor.condition_branches" :key="index" class="condition-branch-row">
          <label>When <JsonEditor v-model="branch.when_json" /></label>
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
        <label>Advanced Wait JSON <JsonEditor v-model="workflows.stepEditor.wait_json" /></label>
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
        <label>Items <JsonEditor v-model="workflows.stepEditor.loop_items_json" /></label>
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'switch'" class="form-section">
        <h3>Switch</h3>
        <label>Value <JsonEditor v-model="workflows.stepEditor.switch_value_json" /></label>
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
          <label>Value <JsonEditor v-model="switchCase.match_json" /></label>
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
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
        <label>Items <JsonEditor v-model="workflows.stepEditor.map_items_json" /></label>
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'emit'" class="form-section">
        <h3>Emit</h3>
        <label>Event Type <input v-model="workflows.stepEditor.emit_event_type" /></label>
        <label>Data <JsonEditor v-model="workflows.stepEditor.emit_data_json" /></label>
        <label>Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'subflow'" class="form-section">
        <h3>Subflow</h3>
        <label>Workflow ID <input v-model.number="workflows.stepEditor.subflow_id" type="number" min="0" /></label>
        <label>Parameters <JsonEditor v-model="workflows.stepEditor.subflow_parameters_json" /></label>
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
        <label>Transitions JSON <JsonEditor v-model="workflows.stepEditor.transitions_json" /></label>
      </section>

      <section v-if="stepRefs.length" class="form-section">
        <h3>Available References</h3>
        <div class="ref-row" v-for="ref in stepRefs" :key="ref.template">
          <code class="ref-template" @click="copyRef(ref.template)">{{ ref.template }}</code>
          <span>{{ ref.label }} -> {{ ref.field }}</span>
        </div>
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
import { computed, onMounted } from "vue";
import { useProvidersStore } from "../../stores/providers";
import { useWorkflowsStore } from "../../stores/workflows";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";
import JsonEditor from "../shared/JsonEditor.vue";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";

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
const targetNodes = computed(() => {
  const nodes: any[] = workflows.workflowDraft.definition?.nodes ?? [];
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});

interface StepRef {
  template: string;
  label: string;
  field: string;
}

const prevStepId = computed<string | null>(() => {
  const nodes: any[] = workflows.workflowDraft.definition?.nodes ?? [];
  const currentId = workflows.selectedStepId;
  if (!currentId) return null;
  const predecessor = nodes.find((node: any) => {
    const transitions = node.transitions ?? {};
    return [transitions.next, transitions.on_success, transitions.on_failure, transitions.on_timeout]
      .map((value) => value?.$node)
      .includes(currentId);
  });
  return predecessor?.id ?? null;
});

const stepRefs = computed<StepRef[]>(() => {
  const refs: StepRef[] = [];
  const nodes: any[] = workflows.workflowDraft.definition?.nodes ?? [];
  const currentId = workflows.selectedStepId;
  const prev = prevStepId.value;
  for (const node of nodes) {
    if (node.kind !== "task" || node.id === currentId) continue;
    const provider = providersStore.providers.find(p => p.name === node.action_name);
    const action = provider?.actions.find(a => a.function_name === node.action_function);
    if (!action?.results?.length) continue;
    for (const result of action.results) {
      const template = node.id === prev
        ? JSON.stringify({ "$ref": { prev: [result.name] } })
        : JSON.stringify({ "$ref": { node: node.id, output: [result.name] } });
      refs.push({ template, label: node.id === prev ? `prev (${node.id})` : node.id, field: `${result.name}: ${result.value_type}` });
    }
  }
  return refs;
});

onMounted(() => {
  if (providersStore.providers.length === 0 && !providersStore.loading) providersStore.fetchProviders();
});

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

function copyRef(template: string) {
  navigator.clipboard.writeText(template).catch(() => {});
}
</script>

<style scoped>
.step-modal {
  width: min(1040px, 100%);
}

.modal-header span,
.result-metadata,
.ref-row span {
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

.result-metadata,
.ref-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.ref-template {
  cursor: pointer;
}
</style>
