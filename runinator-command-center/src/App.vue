<template>
  <LoginView v-if="auth.required && !auth.authenticated" />
  <AppShell v-else>
    <DevView v-if="app.activeTab === 'Dev' && isDesktop" />
    <section v-else-if="app.activeTab === 'Dev'" class="pane">
      <div class="dev-unavailable">
        The Dev environment is only available in the desktop client. It is disabled in the hosted
        web app.
      </div>
    </section>
    <WorkflowsView v-show="app.activeTab === 'Workflows'" />
    <RunsView v-show="app.activeTab === 'Runs'" />
    <ProvidersView v-if="app.activeTab === 'Providers'" />
    <ReplicasView v-if="app.activeTab === 'Replicas'" />
    <ApprovalsView v-if="app.activeTab === 'Approvals'" />
    <ArtifactsView v-if="app.activeTab === 'Artifacts'" />
    <NotificationsView v-if="app.activeTab === 'Notifications'" />
    <EventsView v-if="app.activeTab === 'Events'" />
    <ExternalItemsView v-if="app.activeTab === 'ExternalItems'" />
    <GatesView v-if="app.activeTab === 'Gates'" />
    <SecretsView v-if="app.activeTab === 'Configs'" setting-kind="config" />
    <SecretsView v-if="app.activeTab === 'Secrets'" setting-kind="secret" />
    <OrganizationView v-if="app.activeTab === 'Organization'" />
    <OrgResourcesView v-if="app.activeTab === 'OrgResources'" />
    <AdminSettingsView v-if="app.activeTab === 'AdminSettings'" />
    <PermissionsView v-if="app.activeTab === 'Permissions'" />
    <DeadLettersView v-if="app.activeTab === 'DeadLetters'" />
    <AuditLogView v-if="app.activeTab === 'AuditLog'" />
  </AppShell>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
import { getServiceStatus, startServiceDiscovery } from "./api/commandCenterApi";
import { pingBackendHealth, wsBaseUrl } from "./api/httpRuntime";
import { isTauriRuntime, listenTauri } from "./api/tauriRuntime";
import AppShell from "./components/shell/AppShell.vue";
import { useEventStream } from "./composables/useEventStream";
import { useUrlSync } from "./composables/useUrlSync";
import { endpointForTab, isResourceTab, useAppStore } from "./stores/app";
import { useAuthStore } from "./stores/auth";
import LoginView from "./views/LoginView.vue";
import { useArtifactsStore } from "./stores/artifacts";
import { useNotificationsStore } from "./stores/notifications";
import { useResourcesStore } from "./stores/resources";
import { useOrgsStore } from "./stores/orgs";
import { useSecretsStore } from "./stores/secrets";
import { useWorkflowsStore } from "./stores/workflows";
import { useProvidersStore } from "./stores/providers";
import { usePermissionsStore } from "./stores/permissions";
import { useAdminSettingsStore } from "./stores/adminSettings";
import { useDisplayPreferencesStore } from "./stores/displayPreferences";
import { useGatesStore } from "./stores/gates";
import RunsView from "./views/RunsView.vue";
import ProvidersView from "./views/ProvidersView.vue";
import ReplicasView from "./views/ReplicasView.vue";
import DevView from "./views/DevView.vue";
import WorkflowsView from "./views/WorkflowsView.vue";
import ApprovalsView from "./views/ApprovalsView.vue";
import ArtifactsView from "./views/ArtifactsView.vue";
import NotificationsView from "./views/NotificationsView.vue";
import EventsView from "./views/EventsView.vue";
import ExternalItemsView from "./views/ExternalItemsView.vue";
import GatesView from "./views/GatesView.vue";
import SecretsView from "./views/SecretsView.vue";
import PermissionsView from "./views/PermissionsView.vue";
import OrganizationView from "./views/OrganizationView.vue";
import OrgResourcesView from "./views/OrgResourcesView.vue";
import AdminSettingsView from "./views/AdminSettingsView.vue";
import DeadLettersView from "./views/DeadLettersView.vue";
import AuditLogView from "./views/AuditLogView.vue";

const app = useAppStore();
const auth = useAuthStore();
const isDesktop = isTauriRuntime();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const orgs = useOrgsStore();
const artifacts = useArtifactsStore();
const notifications = useNotificationsStore();
const secrets = useSecretsStore();
const providers = useProvidersStore();
const permissions = usePermissionsStore();
const adminSettings = useAdminSettingsStore();
const gates = useGatesStore();
// initialize early so the theme data-theme attribute is set before first render.
useDisplayPreferencesStore();
useEventStream();
// keep the URL hash in sync with the active tab + selected workflow/run (deep links, back/forward).
useUrlSync();

let unlistenUrl: (() => void) | undefined;
let unlistenError: (() => void) | undefined;
let replicaRefreshTimer = 0;
let healthPollTimer = 0;
let tenantRefreshId = 0;

onMounted(async () => {
  unlistenUrl = await listenTauri("service-url-changed", (event) => {
    const payload = event.payload as { service_url?: string | null } | null;
    void handleServiceUrlChanged(payload?.service_url ?? null);
  });
  unlistenError = await listenTauri("service-discovery-error", (event) => {
    app.setError(String(event.payload));
    app.initialLoading = false;
  });

  if (!isTauriRuntime()) {
    // web mode: same-origin (proxied to runinator-ws via nginx) or
    // VITE_RUNINATOR_WS_URL override for local dev. No Tauri discovery dance.
    const baseUrl = wsBaseUrl();
    app.setServiceUrl(baseUrl || null);

    if (baseUrl) {
      await auth.init();

      if (auth.authenticated) {
        try {
          await refreshBackendState(true);
        } catch (err) {
          app.setError(String(err));
        }
      }
    } else {
      app.setError(
        "No service URL configured. Set VITE_RUNINATOR_WS_URL or serve the SPA from the runinator-command-center-web pod.",
      );
      clearBackendState();
    }

    app.initialLoading = false;

    if (baseUrl) {
      startHealthPoll();
    }

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
      await auth.init();

      if (auth.authenticated) {
        await refreshBackendState(true);
      }
    } else {
      app.setError(
        "No Runinator service discovered. Ensure the web service is running and accessible.",
      );
      clearBackendState();
    }
  } catch (err) {
    app.setError(String(err));
  } finally {
    app.initialLoading = false;
  }

  await refreshServiceStatus();
  replicaRefreshTimer = window.setInterval(() => {
    if (!app.serviceUrl) {
      return;
    }

    void app.refreshReplicas().catch(() => undefined);
  }, 15000);
});

// after a successful login, hydrate the backend state that was skipped while unauthenticated.
watch(
  () => auth.authenticated,
  (authenticated) => {
    if (authenticated && app.serviceUrl) {
      void refreshBackendState(true);
    }
  },
);

watch(
  () => orgs.activeOrgId,
  (orgId, previousOrgId) => {
    if (!orgId || orgId === previousOrgId || !app.serviceUrl || !auth.authenticated) {
      return;
    }

    void refreshTenantScopedState();
  },
);

watch(
  () => app.activeTab,
  (tab) => {
    // reset the shared search box so a query typed on one tab doesn't silently filter the next.
    app.searchQuery = "";

    if (tab === "Workflows" && !workflows.isDirty) {
      void workflows.refreshWorkflows();
    }

    if (tab === "Runs") {
      void workflows.fetchRecentWorkflowRuns();
    }

    if (tab === "Replicas") {
      void app.refreshReplicas().catch(() => undefined);
    }

    if (tab === "Configs") {
      void secrets.refreshSecrets();
    }

    if (tab === "Secrets") {
      void secrets.refreshSecrets();
    }

    if (tab === "AdminSettings") {
      void adminSettings.refresh();
    }

    if (tab === "Artifacts") {
      void artifacts.refreshArtifacts();
    }

    if (tab === "Notifications") {
      void notifications.refreshNotifications();
    }

    if (tab === "Permissions") {
      void permissions.refreshAll();
    }

    if (isResourceTab(tab)) {
      const endpoint = endpointForTab(tab);

      if (endpoint) {
        void resources.refreshResourcesFor(endpoint);
      }
    }
  },
);

watch(
  () => [workflows.workflows.length, resources.resourceRecords.length, secrets.secrets.length],
  () => {
    void refreshServiceStatus();
  },
);

async function refreshServiceStatus() {
  if (!isTauriRuntime()) {
    return;
  }

  const status = await getServiceStatus().catch(() => null);

  if (!status) {
    return;
  }

  app.setServiceUrl(status.service_url);

  if (!status.service_url) {
    clearBackendState();
  }
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
  gates.clearGates();
  adminSettings.clear();
  providers.clearProviders();
  permissions.clearPermissions();
  orgs.clear();
  app.clearReplicaState();
}

function clearTenantScopedState() {
  workflows.clearServiceState({ discardDraft: true });
  resources.clearResources();
  artifacts.clearArtifacts();
  notifications.clearNotifications();
  secrets.clearSecrets();
  permissions.clearPermissions();
  gates.clearGates();
  providers.clearProviders();
  app.clearReplicaState();
}

async function refreshTenantScopedState() {
  const refreshId = ++tenantRefreshId;
  clearTenantScopedState();
  await Promise.all([
    workflows.refreshWorkflows().catch(() => undefined),
    workflows.fetchRecentWorkflowRuns().catch(() => undefined),
    resources.refreshResources().catch(() => undefined),
    artifacts.refreshArtifacts().catch(() => undefined),
    notifications.refreshNotifications().catch(() => undefined),
    secrets.refreshSecrets().catch(() => undefined),
    permissions.refreshAll().catch(() => undefined),
    gates.refreshGates().catch(() => undefined),
    providers.fetchProviders().catch(() => undefined),
    app.refreshReplicas().catch(() => undefined),
  ]);

  if (refreshId === tenantRefreshId && orgs.activeOrg) {
    app.setStatus(`Active organization: ${orgs.activeOrg.name}`);
  }
}

async function refreshBackendState(refreshProviders: boolean) {
  await Promise.all([
    workflows.refreshWorkflows().catch(() => undefined),
    workflows.fetchRecentWorkflowRuns().catch(() => undefined),
    resources.refreshResources().catch(() => undefined),
    notifications.refreshNotifications().catch(() => undefined),
    secrets.refreshSecrets().catch(() => undefined),
    gates.refreshGates().catch(() => undefined),
    app.refreshReplicas().catch(() => undefined),
    orgs.refresh().catch(() => undefined),
    refreshProviders ? providers.fetchProviders().catch(() => undefined) : Promise.resolve(),
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

// web mode has no Tauri discovery loop, so poll the public /health endpoint to detect an idle
// outage (proxy/backend down) or recovery even when the user is not triggering requests.
function startHealthPoll() {
  if (healthPollTimer) {
    return;
  }

  healthPollTimer = window.setInterval(async () => {
    const healthy = await pingBackendHealth();

    if (healthy) {
      app.markBackendReachable();
    } else {
      app.markBackendUnreachable();
    }
  }, 10000);
}

onBeforeUnmount(() => {
  unlistenUrl?.();
  unlistenError?.();
  window.clearInterval(replicaRefreshTimer);
  window.clearInterval(healthPollTimer);
  app.dispose();
});
</script>

<style scoped>
.dev-unavailable {
  color: var(--text-muted);
  padding: 14px 0;
}
</style>
