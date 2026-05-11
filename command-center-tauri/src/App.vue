<template>
  <AppShell>
    <TasksView v-show="app.activeTab === 'Tasks'" />
    <RunsView v-show="app.activeTab === 'Runs'" />
    <WorkflowsView v-show="app.activeTab === 'Workflows'" />
    <ResourcesView v-show="app.activeTab === 'Resources'" />
  </AppShell>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
import { getServiceStatus, startServiceDiscovery } from "./api/commandCenterApi";
import { isTauriRuntime, listenTauri } from "./api/tauriRuntime";
import AppShell from "./components/shell/AppShell.vue";
import { useAutoRefresh } from "./composables/useAutoRefresh";
import { useAppStore } from "./stores/app";
import { useResourcesStore } from "./stores/resources";
import { useTasksStore } from "./stores/tasks";
import { useWorkflowsStore } from "./stores/workflows";
import TasksView from "./views/TasksView.vue";
import RunsView from "./views/RunsView.vue";
import WorkflowsView from "./views/WorkflowsView.vue";
import ResourcesView from "./views/ResourcesView.vue";

const app = useAppStore();
const tasks = useTasksStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
useAutoRefresh();

let unlistenUrl: (() => void) | undefined;
let unlistenError: (() => void) | undefined;

onMounted(async () => {
  unlistenUrl = await listenTauri<{ service_url: string | null }>("service-url-changed", (event) => {
    app.serviceUrl = event.payload.service_url;
    tasks.refreshTasks();
    workflows.refreshWorkflows();
    resources.refreshResources();
  });
  unlistenError = await listenTauri<string>("service-discovery-error", (event) => app.setError(event.payload));
  if (!isTauriRuntime()) {
    app.setError("Tauri runtime unavailable. Use `pnpm --dir command-center-tauri tauri dev` to connect this UI to Runinator.");
    return;
  }
  const status = await getServiceStatus();
  app.serviceUrl = status.service_url;
  await startServiceDiscovery();
  await Promise.all([tasks.refreshTasks(), workflows.refreshWorkflows(), resources.refreshResources()]);
});

watch(
  () => app.activeTab,
  (tab) => {
    if (tab === "Workflows") workflows.refreshWorkflows();
    if (tab === "Runs") tasks.refreshRunsForSelectedTask();
    if (tab === "Resources") resources.refreshResources();
  }
);

onBeforeUnmount(() => {
  unlistenUrl?.();
  unlistenError?.();
  app.dispose();
});
</script>
