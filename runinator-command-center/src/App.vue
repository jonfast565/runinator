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
import { getPlatformAdapter } from "./core/platform";
import { pingBackendHealth } from "./core/api/httpRuntime";
import AppShell from "./ui/components/shell/AppShell.vue";
import { useBreakpoint } from "./ui/composables/useBreakpoint";
import { useEventStream } from "./ui/composables/useEventStream";
import { useUrlSync } from "./ui/composables/useUrlSync";
import { endpointForTab, isResourceTab, useAppStore } from "./ui/adapters/pinia/app";
import { useAuthStore } from "./ui/adapters/pinia/auth";
import LoginView from "./ui/views/LoginView.vue";
import { useArtifactsStore } from "./ui/adapters/pinia/artifacts";
import { useNotificationsStore } from "./ui/adapters/pinia/notifications";
import { useResourcesStore } from "./ui/adapters/pinia/resources";
import { useOrgsStore } from "./ui/adapters/pinia/orgs";
import { useSecretsStore } from "./ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "./ui/adapters/pinia/workflows";
import { useProvidersStore } from "./ui/adapters/pinia/providers";
import { usePermissionsStore } from "./ui/adapters/pinia/permissions";
import { useAdminSettingsStore } from "./ui/adapters/pinia/adminSettings";
import { useDisplayPreferencesStore } from "./ui/adapters/pinia/displayPreferences";
import { useGatesStore } from "./ui/adapters/pinia/gates";
import RunsView from "./ui/views/RunsView.vue";
import ProvidersView from "./ui/views/ProvidersView.vue";
import ReplicasView from "./ui/views/ReplicasView.vue";
import DevView from "./ui/views/DevView.vue";
import WorkflowsView from "./ui/views/WorkflowsView.vue";
import ApprovalsView from "./ui/views/ApprovalsView.vue";
import ArtifactsView from "./ui/views/ArtifactsView.vue";
import NotificationsView from "./ui/views/NotificationsView.vue";
import EventsView from "./ui/views/EventsView.vue";
import ExternalItemsView from "./ui/views/ExternalItemsView.vue";
import GatesView from "./ui/views/GatesView.vue";
import SecretsView from "./ui/views/SecretsView.vue";
import PermissionsView from "./ui/views/PermissionsView.vue";
import OrganizationView from "./ui/views/OrganizationView.vue";
import OrgResourcesView from "./ui/views/OrgResourcesView.vue";
import AdminSettingsView from "./ui/views/AdminSettingsView.vue";
import DeadLettersView from "./ui/views/DeadLettersView.vue";
import AuditLogView from "./ui/views/AuditLogView.vue";

const app = useAppStore();
const auth = useAuthStore();
const platform = getPlatformAdapter();
const discovery = platform.serviceDiscovery;
const isDesktop = discovery.isDesktop();
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
// track viewport size and publish it as document[data-viewport] for css + layout logic.
useBreakpoint();
useEventStream();
// keep the URL hash in sync with the active tab + selected workflow/run (deep links, back/forward).
useUrlSync();

let unlistenUrl: (() => void) | undefined;
let unlistenError: (() => void) | undefined;
let replicaRefreshTimer = 0;
let healthPollTimer = 0;
let tenantRefreshId = 0;

onMounted(async () => {
  unlistenUrl = await discovery.listenServiceUrlChanged((serviceUrl) => {
    void handleServiceUrlChanged(serviceUrl);
  });
  unlistenError = await discovery.listenDiscoveryError((message) => {
    app.setError(message);
    app.initialLoading = false;
  });

  if (!discovery.isDesktop()) {
    const baseUrl = discovery.webServiceUrl();
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
    const [status] = await Promise.all([discovery.getInitialStatus(), discovery.startDiscovery()]);
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
  if (!discovery.isDesktop()) {
    return;
  }

  const status = await discovery.getInitialStatus().catch(() => null);

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
    const status = await discovery.getInitialStatus().catch(() => null);

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
