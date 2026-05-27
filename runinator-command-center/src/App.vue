<template>
  <AppShell>
    <WorkflowsView v-show="app.activeTab === 'Workflows'" />
    <RunsView v-show="app.activeTab === 'Runs'" />
    <ApprovalsView v-if="app.activeTab === 'Approvals'" />
    <ArtifactsView v-if="app.activeTab === 'Artifacts'" />
    <NotificationsView v-if="app.activeTab === 'Notifications'" />
    <FeedbackView v-if="app.activeTab === 'Feedback'" />
    <EventsView v-if="app.activeTab === 'Events'" />
    <ExternalItemsView v-if="app.activeTab === 'ExternalItems'" />
    <ChangeSetsView v-if="app.activeTab === 'ChangeSets'" />
    <WorkspacesView v-if="app.activeTab === 'Workspaces'" />
    <GatesView v-if="app.activeTab === 'Gates'" />
    <SecretsView v-show="app.activeTab === 'Secrets'" />
  </AppShell>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
import { getServiceStatus, startServiceDiscovery } from "./api/commandCenterApi";
import { wsBaseUrl } from "./api/httpRuntime";
import { isTauriRuntime, listenTauri } from "./api/tauriRuntime";
import AppShell from "./components/shell/AppShell.vue";
import { useEventStream } from "./composables/useEventStream";
import { endpointForTab, isResourceTab, useAppStore } from "./stores/app";
import { useArtifactsStore } from "./stores/artifacts";
import { useNotificationsStore } from "./stores/notifications";
import { useResourcesStore } from "./stores/resources";
import { useSecretsStore } from "./stores/secrets";
import { useWorkflowsStore } from "./stores/workflows";
import { useProvidersStore } from "./stores/providers";
import RunsView from "./views/RunsView.vue";
import WorkflowsView from "./views/WorkflowsView.vue";
import ApprovalsView from "./views/ApprovalsView.vue";
import ArtifactsView from "./views/ArtifactsView.vue";
import NotificationsView from "./views/NotificationsView.vue";
import FeedbackView from "./views/FeedbackView.vue";
import EventsView from "./views/EventsView.vue";
import ExternalItemsView from "./views/ExternalItemsView.vue";
import ChangeSetsView from "./views/ChangeSetsView.vue";
import WorkspacesView from "./views/WorkspacesView.vue";
import GatesView from "./views/GatesView.vue";
import SecretsView from "./views/SecretsView.vue";

const app = useAppStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const artifacts = useArtifactsStore();
const notifications = useNotificationsStore();
const secrets = useSecretsStore();
const providers = useProvidersStore();
useEventStream();

let unlistenUrl: (() => void) | undefined;
let unlistenError: (() => void) | undefined;

onMounted(async () => {
  unlistenUrl = await listenTauri<{ service_url?: string | null } | null>("service-url-changed", (event) => {
    void handleServiceUrlChanged(event.payload?.service_url ?? null);
  });
  unlistenError = await listenTauri<string>("service-discovery-error", (event) => {
    app.setError(event.payload);
    app.initialLoading = false;
  });
  if (!isTauriRuntime()) {
    // web mode: same-origin (proxied to runinator-ws via nginx) or
    // VITE_RUNINATOR_WS_URL override for local dev. No Tauri discovery dance.
    const baseUrl = wsBaseUrl();
    app.setServiceUrl(baseUrl || null);
    if (baseUrl) {
      try {
        await refreshBackendState(true);
      } catch (err) {
        app.setError(String(err));
      }
    } else {
      app.setError("No service URL configured. Set VITE_RUNINATOR_WS_URL or serve the SPA from the runinator-command-center-web pod.");
      clearBackendState();
    }
    app.initialLoading = false;
    return;
  }
  try {
    const [status] = await Promise.all([getServiceStatus(), startServiceDiscovery()]);
    console.info("[command-center] Initial service status", status);
    app.setServiceUrl(status.service_url);
    if (!status.service_url) {
      await waitForConcreteServiceUrl();
    }
    if (app.serviceUrl) {
      await refreshBackendState(true);
    } else {
      app.setError("No Runinator service discovered. Ensure the web service is running and accessible.");
      clearBackendState();
    }
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
    if (tab === "Runs") workflows.fetchRecentWorkflowRuns();
    if (tab === "Secrets") secrets.refreshSecrets();
    if (tab === "Artifacts") artifacts.refreshArtifacts();
    if (tab === "Notifications") notifications.refreshNotifications();
    if (isResourceTab(tab)) {
      const endpoint = endpointForTab(tab);
      if (endpoint) void resources.refreshResourcesFor(endpoint);
    }
  }
);

watch(
  () => [workflows.workflows.length, resources.resourceRecords.length, secrets.secrets.length],
  () => {
    refreshServiceStatus();
  }
);

async function refreshServiceStatus() {
  if (!isTauriRuntime()) return;
  const status = await getServiceStatus().catch(() => null);
  if (!status) return;
  app.setServiceUrl(status.service_url);
  if (!status.service_url) clearBackendState();
}

async function handleServiceUrlChanged(serviceUrl: string | null) {
  const previousServiceUrl = app.serviceUrl;
  app.setServiceUrl(serviceUrl);
  app.initialLoading = false;
  if (!serviceUrl) {
    clearBackendState();
    return;
  }
  await refreshBackendState(previousServiceUrl !== serviceUrl || providers.providers.length === 0);
}

function clearBackendState() {
  workflows.clearServiceState();
  resources.clearResources();
  artifacts.clearArtifacts();
  notifications.clearNotifications();
  secrets.clearSecrets();
  providers.clearProviders();
}

async function refreshBackendState(refreshProviders: boolean) {
  await Promise.all([
    workflows.refreshWorkflows().catch(() => {}),
    workflows.fetchRecentWorkflowRuns().catch(() => {}),
    resources.refreshResources().catch(() => {}),
    notifications.refreshNotifications().catch(() => {}),
    secrets.refreshSecrets().catch(() => {}),
    refreshProviders ? providers.fetchProviders().catch(() => {}) : Promise.resolve()
  ]);
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
