<template>
  <AppShell>
    <TasksView v-show="app.activeTab === 'Tasks'" />
    <RunsView v-show="app.activeTab === 'Runs'" />
    <WorkflowsView v-show="app.activeTab === 'Workflows'" />
    <ResourcesView v-show="app.activeTab === 'Resources'" />
    <SecretsView v-show="app.activeTab === 'Secrets'" />
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
import { useSecretsStore } from "./stores/secrets";
import { useTasksStore } from "./stores/tasks";
import { useWorkflowsStore } from "./stores/workflows";
import { useProvidersStore } from "./stores/providers";
import TasksView from "./views/TasksView.vue";
import RunsView from "./views/RunsView.vue";
import WorkflowsView from "./views/WorkflowsView.vue";
import ResourcesView from "./views/ResourcesView.vue";
import SecretsView from "./views/SecretsView.vue";

const app = useAppStore();
const tasks = useTasksStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();
const providers = useProvidersStore();
useAutoRefresh();

let unlistenUrl: (() => void) | undefined;
let unlistenError: (() => void) | undefined;

onMounted(async () => {
  unlistenUrl = await listenTauri<{ service_url: string | null }>("service-url-changed", (event) => {
    app.setServiceUrl(event.payload.service_url);
    app.markBackendReachable();
    app.initialLoading = false;
    Promise.all([
      tasks.refreshTasks(),
      workflows.refreshWorkflows(),
      resources.refreshResources(),
      secrets.refreshSecrets()
    ]);
  });
  unlistenError = await listenTauri<string>("service-discovery-error", (event) => {
    app.setError(event.payload);
    app.initialLoading = false;
  });
  if (!isTauriRuntime()) {
    app.setError("Tauri runtime unavailable. Use `pnpm --dir command-center-tauri tauri dev` to connect this UI to Runinator.");
    app.initialLoading = false;
    return;
  }
  try {
    const [status] = await Promise.all([getServiceStatus(), startServiceDiscovery()]);
    app.setServiceUrl(status.service_url);
    if (!status.service_url) {
      await waitForConcreteServiceUrl();
    }
    await Promise.all([
      tasks.refreshTasks().catch(() => {}),
      workflows.refreshWorkflows().catch(() => {}),
      resources.refreshResources().catch(() => {}),
      secrets.refreshSecrets().catch(() => {}),
      providers.fetchProviders().catch(() => {})
    ]);
  } catch (err) {
    app.setError(String(err));
  } finally {
    app.initialLoading = false;
  }
  await refreshServiceStatus();
});

watch(
  () => app.activeTab,
  (tab) => {
    if (tab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
    if (tab === "Runs") tasks.refreshRunsForSelectedTask();
    if (tab === "Resources") resources.refreshResources();
    if (tab === "Secrets") secrets.refreshSecrets();
  }
);

watch(
  () => [tasks.tasks.length, workflows.workflows.length, resources.resourceRecords.length, secrets.secrets.length],
  () => {
    refreshServiceStatus();
  }
);

async function refreshServiceStatus() {
  if (!isTauriRuntime()) return;
  const status = await getServiceStatus().catch(() => null);
  app.setServiceUrl(status?.service_url);
}

async function waitForConcreteServiceUrl(timeoutMs = 5000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const status = await getServiceStatus().catch(() => null);
    if (status?.service_url) {
      app.setServiceUrl(status.service_url);
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 250));
  }
}

onBeforeUnmount(() => {
  unlistenUrl?.();
  unlistenError?.();
  app.dispose();
});
</script>
