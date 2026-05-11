<template>
  <div class="modal-backdrop">
    <form class="modal task-modal" @submit.prevent="tasks.submitTask">
      <header class="modal-header">
        <h2>{{ tasks.editingTaskCreating ? "New Task" : "Edit Task" }}</h2>
        <button type="button" @click="tasks.closeTaskEditor">Close</button>
      </header>

      <section class="form-section">
        <h3>Identity</h3>
        <div class="form-grid">
          <label>Name <input v-model="tasks.taskDraft.name" /></label>
          <label>
            Action Name
            <select :value="tasks.taskDraft.action_name" @change="onActionNameChange">
              <option value="" disabled>Select action name</option>
              <option v-if="selectedProviderMissing" :value="tasks.taskDraft.action_name">
                {{ tasks.taskDraft.action_name }} (unavailable)
              </option>
              <option v-for="p in providersStore.providers" :key="p.name" :value="p.name">
                {{ p.name }}
              </option>
            </select>
          </label>
          <label>
            Action Function
            <select v-model="tasks.taskDraft.action_function" :disabled="!currentProvider" @change="applyParameterDefaults">
              <option value="" disabled>{{ currentProvider ? "Select action function" : "Select action name first" }}</option>
              <option v-if="selectedActionMissing" :value="tasks.taskDraft.action_function">
                {{ tasks.taskDraft.action_function }} (unavailable)
              </option>
              <option v-for="action in currentActions" :key="action.function_name" :value="action.function_name">
                {{ action.function_name }}
              </option>
            </select>
          </label>
          <label class="checkbox"><input v-model="tasks.taskDraft.mcp_enabled" type="checkbox" /> MCP Enabled</label>
        </div>
      </section>

      <section class="form-section">
        <h3>Schedule</h3>
        <div class="form-grid">
          <label>Cron <input v-model="tasks.taskDraft.cron_schedule" /></label>
          <label>Timeout <input v-model.number="tasks.taskDraft.timeout" type="number" min="1" /></label>
          <label class="checkbox"><input v-model="tasks.taskDraft.enabled" type="checkbox" /> Enabled</label>
        </div>
      </section>

      <section class="form-section">
        <h3>Default Parameters</h3>
        <TypedParameterEditor v-model="defaultParameters" :parameters="selectedAction?.parameters ?? []" :credential-scopes="currentProvider?.metadata.credential_scopes ?? []" />
        <p v-if="selectedAction?.results?.length" class="metadata-hint">
          Results:
          <span v-for="result in selectedAction.results" :key="result.name">
            {{ result.name }} ({{ result.value_type }})
          </span>
        </p>
      </section>

      <section class="form-section json-section">
        <h3>Advanced</h3>
        <label>Default Parameters (JSON) <JsonEditor v-model="tasks.taskJson.default_parameters" /></label>
        <label>Metadata <JsonEditor v-model="tasks.taskJson.metadata" /></label>
      </section>

      <p v-if="tasks.taskEditorError" class="error">{{ tasks.taskEditorError }}</p>
      <div class="modal-actions">
        <button type="button" @click="tasks.closeTaskEditor">Cancel</button>
        <button type="submit">Save</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { useTasksStore } from "../../stores/tasks";
import { useProvidersStore } from "../../stores/providers";
import JsonEditor from "./JsonEditor.vue";
import TypedParameterEditor from "./TypedParameterEditor.vue";
import { computed, onMounted } from "vue";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";

const tasks = useTasksStore();
const providersStore = useProvidersStore();

const currentProvider = computed(() =>
  providersStore.providers.find(p => p.name === tasks.taskDraft.action_name)
);
const currentActions = computed(() => currentProvider.value?.actions ?? []);
const selectedAction = computed(() =>
  currentActions.value.find(action => action.function_name === tasks.taskDraft.action_function) ?? null
);
const defaultParameters = computed({
  get: () => parseObject(tasks.taskJson.default_parameters, {}),
  set: (value) => {
    tasks.taskJson.default_parameters = pretty(value);
  }
});
const selectedProviderMissing = computed(() =>
  Boolean(tasks.taskDraft.action_name && !currentProvider.value)
);
const selectedActionMissing = computed(() =>
  Boolean(
    tasks.taskDraft.action_function &&
      currentProvider.value &&
      !currentActions.value.some(action => action.function_name === tasks.taskDraft.action_function)
  )
);

onMounted(() => {
  if (providersStore.providers.length === 0 && !providersStore.loading) {
    providersStore.fetchProviders();
  }
});

function onActionNameChange(e: Event) {
  const name = (e.target as HTMLSelectElement).value;
  tasks.taskDraft.action_name = name;
  const provider = providersStore.providers.find(p => p.name === name);
  tasks.taskDraft.action_function = provider?.actions[0]?.function_name ?? "";
  applyParameterDefaults();
}

function applyParameterDefaults() {
  const action = selectedAction.value;
  if (!action) return;
  const current = parseObject(tasks.taskJson.default_parameters, {});
  const next = { ...current };
  for (const parameter of action.parameters ?? []) {
    if (next[parameter.name] === undefined && parameter.default_value !== undefined) {
      next[parameter.name] = parameter.default_value;
    }
  }
  tasks.taskJson.default_parameters = pretty(next);
}
</script>

<style scoped>
.metadata-hint {
  color: #66717e;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
}
</style>
