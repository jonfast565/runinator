<template>
  <header class="topbar">
    <button
      class="btn nav-hamburger"
      :class="{ 'is-open': app.mobileNavOpen }"
      :aria-label="app.mobileNavOpen ? 'Close navigation' : 'Open navigation'"
      :aria-expanded="app.mobileNavOpen"
      :title="app.mobileNavOpen ? 'Close navigation' : 'Open navigation'"
      :disabled="app.interactionsDisabled"
      @click="app.toggleMobileNav()"
    >
      <span class="hamburger-box" aria-hidden="true">
        <span class="hamburger-bar"></span>
        <span class="hamburger-bar"></span>
        <span class="hamburger-bar"></span>
      </span>
    </button>
    <div class="view-title">
      <h1>{{ headingFor(app.activeTab) }}</h1>
      <span v-if="app.loading && app.opLabel" class="view-status loading">
        <LoadingSpinner size="sm" :label="app.opLabel" />
        {{ app.opLabel }}…
      </span>
      <span v-else>{{ activeSubtitle }}</span>
    </div>
    <div v-if="searchPlaceholder" class="toolbar-search">
      <input
        id="global-search"
        v-model="app.searchQuery"
        :disabled="app.interactionsDisabled"
        :placeholder="searchPlaceholder"
      />
    </div>
    <div class="actions">
      <select
        v-if="orgs.hasOrgs"
        class="org-select"
        :value="orgs.activeOrgId ?? ''"
        title="Active organization"
        :disabled="app.interactionsDisabled"
        @change="onSwitchOrg"
      >
        <option value="" disabled>Org…</option>
        <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
          {{ m.org.name }}
        </option>
      </select>
      <ConnectionStrip />
      <button
        v-if="!app.isRealtime"
        class="btn"
        aria-label="Refresh"
        title="Refresh"
        :disabled="app.interactionsDisabled || app.loading"
        @click="$emit('refresh')"
      >
        <LoadingSpinner v-if="app.loading" size="sm" label="Refreshing" />
        <Icon v-else name="refresh" />
        <span>Refresh</span>
      </button>
      <button
        v-if="app.activeTab === 'Workflows'"
        class="btn btn-primary"
        aria-label="Run workflow"
        title="Run workflow"
        :disabled="app.interactionsDisabled || !workflows.canRunWorkflow || startingRun"
        @click="workflows.runSelectedWorkflow()"
      >
        <LoadingSpinner v-if="startingRun" size="sm" label="Starting run" />
        <Icon v-else name="play" />
        <span>{{ startingRun ? "Starting…" : "Run Workflow" }}</span>
      </button>
      <UserMenu v-if="auth.user" />
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from "vue";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import ConnectionStrip from "./ConnectionStrip.vue";
import UserMenu from "./UserMenu.vue";
import { navItemForTab, useAppStore } from "../../../ui/adapters/pinia/app";
import { useAuthStore } from "../../../ui/adapters/pinia/auth";
import { useResourcesStore } from "../../../ui/adapters/pinia/resources";
import { useOrgsStore } from "../../../ui/adapters/pinia/orgs";
import { useSecretsStore } from "../../../ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useOperationLoading } from "../../composables/useOperationLoading";
import type { AppTab } from "../../../core/navigation/app";

defineEmits<{ refresh: [] }>();

const app = useAppStore();
const auth = useAuthStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const orgs = useOrgsStore();
const secrets = useSecretsStore();
const { isLoading: startingRun } = useOperationLoading("Running workflow", { prefix: true });

function onSwitchOrg(event: Event) {
  const orgId = (event.target as HTMLSelectElement).value;

  if (orgId) {
    void orgs.setActive(orgId);
  }
}

function headingFor(tab: AppTab): string {
  return navItemForTab(tab)?.label ?? tab;
}

// only show the global search box on tabs whose list actually consumes app.searchQuery.
const searchPlaceholder = computed(() => navItemForTab(app.activeTab)?.searchPlaceholder ?? "");

const activeSubtitle = computed(() => {
  switch (app.activeTab) {
    case "Runs":
      return workflows.selectedWorkflowRunId
        ? `Run ${workflows.selectedWorkflowRunId}`
        : "Selected workflow runs";
    case "Workflows":
      return workflows.selectedWorkflow?.name ?? `${String(workflows.workflows.length)} workflows`;
    case "Replicas":
      return `${String(app.liveReplicaCount)}/${String(app.replicas.length)} healthy across ${String(app.replicaCounts.webservices)} ws, ${String(app.replicaCounts.workers)} workers, ${String(app.replicaCounts.wakers)} wakers`;
    case "Secrets":
      return `${String(secrets.secrets.length)} secrets`;
    case "Artifacts":
      return "File artifacts attached to runs";
    case "Notifications":
      return "In-app and email notifications";
    default:
      return (
        resources.resources.find(
          (resource) => resource.endpoint === resources.selectedResourceEndpoint,
        )?.label ?? ""
      );
  }
});
</script>
