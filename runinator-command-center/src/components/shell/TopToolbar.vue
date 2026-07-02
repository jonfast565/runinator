<template>
  <header class="topbar">
    <button
      class="btn nav-hamburger"
      aria-label="Open navigation"
      title="Open navigation"
      :disabled="app.interactionsDisabled"
      @click="app.toggleMobileNav()"
    >
      <Icon name="list" :size="18" />
    </button>
    <div class="view-title">
      <h1>{{ headingFor(app.activeTab) }}</h1>
      <span>{{ activeSubtitle }}</span>
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
        :disabled="app.interactionsDisabled"
        @click="$emit('refresh')"
      >
        <Icon name="refresh" />
        <span>Refresh</span>
      </button>
      <button
        v-if="app.activeTab === 'Workflows'"
        class="btn btn-primary"
        aria-label="Run workflow"
        title="Run workflow"
        :disabled="app.interactionsDisabled || !workflows.canRunWorkflow"
        @click="workflows.runSelectedWorkflow()"
      >
        <Icon name="play" />
        <span>Run Workflow</span>
      </button>
      <UserMenu v-if="auth.user" />
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from "vue";
import Icon from "../shared/Icon.vue";
import ConnectionStrip from "./ConnectionStrip.vue";
import UserMenu from "./UserMenu.vue";
import { navItemForTab, useAppStore } from "../../stores/app";
import { useAuthStore } from "../../stores/auth";
import { useResourcesStore } from "../../stores/resources";
import { useOrgsStore } from "../../stores/orgs";
import { useSecretsStore } from "../../stores/secrets";
import { useWorkflowsStore } from "../../stores/workflows";
import type { AppTab } from "../../types/app";

defineEmits<{ refresh: [] }>();

const app = useAppStore();
const auth = useAuthStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const orgs = useOrgsStore();
const secrets = useSecretsStore();

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
