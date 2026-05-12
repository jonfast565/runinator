<template>
  <div class="step-editor">
    <h2>{{ workflows.selectedStepId || "Step" }}</h2>
    <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
    <label>
      Node Kind
      <select v-model="workflows.stepEditor.kind" :disabled="isProtectedNode">
        <option value="start">start</option>
        <option v-for="kind in workflows.workflowNodeKinds" :key="kind" :value="kind">{{ kind }}</option>
        <option value="end">end</option>
      </select>
    </label>

    <label v-if="workflows.stepEditor.kind === 'task'">
      Task
      <select v-model.number="workflows.stepEditor.task_id">
        <option :value="0">(none)</option>
        <option v-for="task in tasksStore.tasks" :key="task.id ?? task.name" :value="task.id">
          {{ task.name }} ({{ task.action_name }}.{{ task.action_function }})
        </option>
      </select>
    </label>
    <div v-if="workflows.stepEditor.kind === 'task' && selectedTask" class="step-task-info">
      <span>{{ selectedTask.action_name }}.{{ selectedTask.action_function }}</span>
      <p v-if="currentProvider?.actions.find(a => a.function_name === selectedTask?.action_function)?.description" class="action-desc">
        {{ currentProvider.actions.find(a => a.function_name === selectedTask?.action_function)?.description }}
      </p>
    </div>
    <label>Needs <input :value="workflows.stepNeeds" disabled /></label>
    <label v-if="workflows.stepEditor.kind === 'task'">Max Attempts <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1" /></label>
    <label v-if="workflows.stepEditor.kind === 'task'">Timeout Seconds <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0" /></label>
    <TypedParameterEditor
      v-if="workflows.stepEditor.kind === 'task' && selectedTask"
      v-model="stepParameters"
      :parameters="selectedAction?.parameters ?? []"
      :credential-scopes="currentProvider?.metadata.credential_scopes ?? []"
    />
    <p v-if="selectedAction?.results?.length" class="result-metadata">
      Results:
      <span v-for="result in selectedAction.results" :key="result.name">
        {{ result.name }} ({{ result.value_type }})
      </span>
    </p>

    <div v-if="workflows.stepEditor.kind === 'approval'" class="form-section">
      <h3>Approval</h3>
      <label>Approval Type <input v-model="workflows.stepEditor.approval_type" /></label>
      <label>Prompt <textarea v-model="workflows.stepEditor.approval_prompt"></textarea></label>
      <div v-if="workflows.selectedNodePendingApproval" class="pending-approval">
        <strong>Pending approval</strong>
        <span>{{ workflows.selectedNodePendingApproval.status }}</span>
        <span v-if="workflows.selectedNodePendingApproval.message">{{ workflows.selectedNodePendingApproval.message }}</span>
      </div>
      <div class="approval-actions" v-if="workflows.selectedNodePendingApproval">
        <button @click="resolveSelectedApproval('approve')">Approve</button>
        <button @click="resolveSelectedApproval('reject')">Reject</button>
      </div>
    </div>

    <div v-if="workflows.stepEditor.kind === 'condition'" class="form-section">
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
        <button @click="workflows.removeConditionBranchEditor(index)">Remove</button>
      </div>
      <button @click="workflows.addConditionBranchEditor">Add Branch</button>
      <label>
        Fallback
        <select v-model="workflows.stepEditor.condition_fallback">
          <option value="">(none)</option>
          <option v-for="node in targetNodes" :key="node.id" :value="node.id">{{ node.id }}</option>
        </select>
      </label>
    </div>

    <label v-if="workflows.stepEditor.kind === 'wait'">Wait JSON <JsonEditor v-model="workflows.stepEditor.wait_json" /></label>
    <label v-if="usesParameters">Advanced Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
    <label>Transitions JSON <JsonEditor v-model="workflows.stepEditor.transitions_json" /></label>

    <div v-if="workflows.selectedStepId" class="transition-helpers">
      <h3>Quick Transitions</h3>
      <div v-for="key in workflows.directTransitionKeys" :key="key" class="transition-field">
        <span>{{ key }}</span>
        <select :value="workflows.getTransition(key)" @change="workflows.setTransition(key, ($event.target as HTMLSelectElement).value)">
          <option value="">(none)</option>
          <option v-for="node in workflows.workflowDraft.definition.nodes" :key="node.id" :value="node.id">
            {{ node.id }}
          </option>
        </select>
      </div>
    </div>

    <div v-if="stepRefs.length" class="ref-builder">
      <h3>Available References</h3>
      <div v-for="ref in stepRefs" :key="ref.template" class="ref-row">
        <code class="ref-template" @click="copyRef(ref.template)" :title="'Click to copy'">{{ ref.template }}</code>
        <span class="ref-desc">{{ ref.label }} → {{ ref.field }}</span>
      </div>
    </div>

    <button :disabled="!workflows.selectedStepId" @click="workflows.applyStepEditor">Apply Step</button>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import { useProvidersStore } from "../../stores/providers";
import { useResourcesStore } from "../../stores/resources";
import { useTasksStore } from "../../stores/tasks";
import JsonEditor from "../shared/JsonEditor.vue";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";
import { computed } from "vue";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";
import type { ApprovalAction } from "../../utils/approvals";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const tasksStore = useTasksStore();
const resourcesStore = useResourcesStore();

const selectedTask = computed(() =>
  tasksStore.tasks.find(t => t.id === workflows.stepEditor.task_id)
);

const currentProvider = computed(() =>
  selectedTask.value ? providersStore.providers.find(p => p.name === selectedTask.value?.action_name) : null
);
const selectedAction = computed(() =>
  currentProvider.value?.actions.find(action => action.function_name === selectedTask.value?.action_function) ?? null
);
const stepParameters = computed({
  get: () => parseObject(workflows.stepEditor.parameters_json, {}),
  set: (value) => {
    workflows.stepEditor.parameters_json = pretty(value);
  }
});
const isProtectedNode = computed(() => ["start", "end"].includes(workflows.selectedNode?.kind ?? ""));
const usesParameters = computed(() => !["start", "end", "wait", "condition"].includes(workflows.stepEditor.kind));
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
    const t = node.transitions ?? {};
    return [t.next, t.on_success, t.on_failure, t.on_timeout]
      .filter(Boolean)
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
    const task = tasksStore.tasks.find(t => t.id === node.task_id);
    if (!task) continue;
    const provider = providersStore.providers.find(p => p.name === task.action_name);
    const action = provider?.actions.find(a => a.function_name === task.action_function);
    if (!action?.results?.length) continue;

    for (const result of action.results) {
      const isPrev = node.id === prev;
      const template = isPrev
        ? JSON.stringify({ "$ref": { prev: [result.name] } })
        : JSON.stringify({ "$ref": { node: node.id, output: [result.name] } });
      refs.push({
        template,
        label: isPrev ? `prev (${node.id})` : node.id,
        field: `${result.name}: ${result.value_type}`,
      });
    }
  }
  return refs;
});

function copyRef(template: string) {
  navigator.clipboard.writeText(template).catch(() => {});
}

async function resolveSelectedApproval(action: ApprovalAction) {
  const detail = workflows.workflowRunDetail;
  const nodeRun = workflows.selectedNodePendingApproval;
  if (!detail || !nodeRun || !workflows.selectedStepId) return;
  await resourcesStore.resolveWorkflowApproval(detail.run.id, workflows.selectedStepId, nodeRun, action);
  await workflows.fetchWorkflowRunDetail(detail.run.id);
}
</script>

<style scoped>
.transition-helpers {
  margin: 12px 0;
  padding: 8px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 4px;
}
.transition-field {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 4px;
  font-size: 12px;
}
.transition-field select {
  width: 120px;
}
.result-metadata {
  color: #66717e;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
}
.condition-branch-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 140px auto;
  gap: 8px;
  align-items: end;
}
.condition-branch-row .json-editor-container {
  min-height: 96px;
}
.pending-approval {
  display: grid;
  gap: 3px;
  border: 1px solid #f1c40f;
  border-radius: 6px;
  background: #fef9e7;
  padding: 8px;
  color: #4b5663;
  font-size: 12px;
}
.approval-actions {
  display: flex;
  gap: 8px;
}
</style>
