<template>
  <header class="topbar">
    <div class="view-title">
      <h1>{{ app.activeTab }}</h1>
      <span>{{ activeSubtitle }}</span>
    </div>
    <div class="toolbar-search">
      <input id="global-search" v-model="app.searchQuery" placeholder="Search" />
    </div>
    <div class="actions">
      <button @click="$emit('refresh')">Refresh</button>
      <button v-if="app.activeTab === 'Tasks' || app.activeTab === 'Runs'" :disabled="!tasks.canRunTask" @click="tasks.runSelectedTask">
        Run Now
      </button>
      <button v-if="app.activeTab === 'Workflows'" :disabled="!workflows.canRunWorkflow" @click="workflows.runSelectedWorkflow()">
        Run Workflow
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useAppStore } from "../../stores/app";
import { resources, useResourcesStore } from "../../stores/resources";
import { useSecretsStore } from "../../stores/secrets";
import { useTasksStore } from "../../stores/tasks";
import { useWorkflowsStore } from "../../stores/workflows";

defineEmits<{ refresh: [] }>();

const app = useAppStore();
const tasks = useTasksStore();
const workflows = useWorkflowsStore();
const resourcesStore = useResourcesStore();
const secrets = useSecretsStore();

const activeSubtitle = computed(() => {
  if (app.activeTab === "Tasks") return tasks.selectedTask?.name ?? `${tasks.scheduledTasks.length} tasks`;
  if (app.activeTab === "Runs") return tasks.selectedRunId ? `Run ${tasks.selectedRunId}` : "Selected task runs";
  if (app.activeTab === "Workflows") return workflows.selectedWorkflow?.name ?? `${workflows.workflows.length} workflows`;
  if (app.activeTab === "Resources") return resources.find((resource) => resource.endpoint === resourcesStore.selectedResourceEndpoint)?.label ?? "Resources";
  return `${secrets.secrets.length} secrets`;
});
</script>
