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
            </select>
          </label>
          <label>Max Attempts <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1" /></label>
          <label>Timeout Seconds <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0" /></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'task' && taskDraft" class="form-section">
        <div class="section-title-row">
          <h3>Workflow Task</h3>
          <label>
            Import Copy
            <select @change="importTaskCopy">
              <option value="">Select task</option>
              <option v-for="task in tasksStore.tasks" :key="task.id ?? task.name" :value="task.id ?? ''">
                {{ task.name }} ({{ task.action_name }}.{{ task.action_function }})
              </option>
            </select>
          </label>
        </div>
        <div class="form-grid">
          <label>Name <input v-model="taskDraft.name" /></label>
          <label>Cron <input v-model="taskDraft.cron_schedule" /></label>
          <label>
            Action Name
            <select :value="taskDraft.action_name" @change="onActionNameChange">
              <option value="" disabled>Select action name</option>
              <option v-if="selectedProviderMissing" :value="taskDraft.action_name">{{ taskDraft.action_name }} (unavailable)</option>
              <option v-for="provider in providersStore.providers" :key="provider.name" :value="provider.name">{{ provider.name }}</option>
            </select>
          </label>
          <label>
            Action Function
            <select v-model="taskDraft.action_function" :disabled="!currentProvider" @change="applyParameterDefaults">
              <option value="" disabled>{{ currentProvider ? "Select action function" : "Select action name first" }}</option>
              <option v-if="selectedActionMissing" :value="taskDraft.action_function">{{ taskDraft.action_function }} (unavailable)</option>
              <option v-for="action in currentActions" :key="action.function_name" :value="action.function_name">{{ action.function_name }}</option>
            </select>
          </label>
          <label>Timeout <input v-model.number="taskDraft.timeout" type="number" min="1" /></label>
          <label class="checkbox"><input v-model="taskDraft.enabled" type="checkbox" /> Enabled</label>
          <label class="checkbox"><input v-model="taskDraft.mcp_enabled" type="checkbox" /> MCP Enabled</label>
        </div>
        <p v-if="selectedAction?.results?.length" class="result-metadata">
          Results:
          <span v-for="result in selectedAction.results" :key="result.name">{{ result.name }} ({{ result.value_type }})</span>
        </p>
      </section>

      <section v-if="workflows.stepEditor.kind === 'task'" class="form-section">
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
        <label>Wait JSON <JsonEditor v-model="workflows.stepEditor.wait_json" /></label>
      </section>

      <section v-if="advancedParameterKind" class="form-section">
        <h3>{{ workflows.stepEditor.kind }} Parameters</h3>
        <label>Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
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
import { useTasksStore } from "../../stores/tasks";
import { useWorkflowsStore } from "../../stores/workflows";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";
import JsonEditor from "../shared/JsonEditor.vue";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";

const workflows = useWorkflowsStore();
const tasksStore = useTasksStore();
const providersStore = useProvidersStore();

const taskDraft = computed(() => {
  if (workflows.stepEditor.kind !== "task" || !workflows.selectedStepId || !workflows.selectedNode) return null;
  return workflows.ensureWorkflowTaskDraft(workflows.selectedStepId, workflows.selectedNode);
});
const currentProvider = computed(() => taskDraft.value ? providersStore.providers.find(provider => provider.name === taskDraft.value?.action_name) : null);
const currentActions = computed(() => currentProvider.value?.actions ?? []);
const selectedAction = computed(() => currentActions.value.find(action => action.function_name === taskDraft.value?.action_function) ?? null);
const selectedProviderMissing = computed(() => Boolean(taskDraft.value?.action_name && !currentProvider.value));
const selectedActionMissing = computed(() =>
  Boolean(taskDraft.value?.action_function && currentProvider.value && !currentActions.value.some(action => action.function_name === taskDraft.value?.action_function))
);
const stepParameters = computed({
  get: () => parseObject(workflows.stepEditor.parameters_json, {}),
  set: (value) => {
    workflows.stepEditor.parameters_json = pretty(value);
  }
});
const isProtectedNode = computed(() => ["start", "end"].includes(workflows.selectedNode?.kind ?? ""));
const targetNodes = computed(() => {
  const nodes: any[] = workflows.workflowDraft.definition?.nodes ?? [];
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});
const advancedParameterKind = computed(() => !["start", "end", "task", "approval", "condition", "wait"].includes(workflows.stepEditor.kind));

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
    const task = workflows.workflowTaskDrafts[node.id] ?? tasksStore.tasks.find(t => t.id === node.task_id);
    if (!task) continue;
    const provider = providersStore.providers.find(p => p.name === task.action_name);
    const action = provider?.actions.find(a => a.function_name === task.action_function);
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
  if (!taskDraft.value) return;
  const name = (event.target as HTMLSelectElement).value;
  taskDraft.value.action_name = name;
  const provider = providersStore.providers.find(item => item.name === name);
  taskDraft.value.action_function = provider?.actions[0]?.function_name ?? "";
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

function importTaskCopy(event: Event) {
  const value = Number((event.target as HTMLSelectElement).value);
  const task = tasksStore.tasks.find(item => item.id === value);
  if (task) workflows.importTaskForSelectedStep(task);
  (event.target as HTMLSelectElement).value = "";
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
