<template>
  <header class="topbar">
    <div class="view-title">
      <h1>{{ headingFor(app.activeTab) }}</h1>
      <span>{{ activeSubtitle }}</span>
    </div>
    <div v-if="searchPlaceholder" class="toolbar-search">
      <input id="global-search" v-model="app.searchQuery" :disabled="app.serviceBlocked" :placeholder="searchPlaceholder" />
    </div>
    <div class="actions">
      <select
        v-if="orgs.hasOrgs"
        class="org-select"
        :value="orgs.activeOrgId ?? ''"
        title="Active organization"
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
        :disabled="app.serviceBlocked"
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
        :disabled="app.serviceBlocked || !workflows.canRunWorkflow"
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
  if (orgId) void orgs.setActive(orgId);
}

function headingFor(tab: AppTab): string {
  return navItemForTab(tab)?.label ?? String(tab);
}

// only show the global search box on tabs whose list actually consumes app.searchQuery.
const searchPlaceholder = computed(() => navItemForTab(app.activeTab)?.searchPlaceholder ?? "");

const activeSubtitle = computed(() => {
  switch (app.activeTab) {
    case "Runs":
      return workflows.selectedWorkflowRunId ? `Run ${workflows.selectedWorkflowRunId}` : "Selected workflow runs";
    case "Workflows":
      return workflows.selectedWorkflow?.name ?? `${workflows.workflows.length} workflows`;
    case "Replicas":
      return `${app.liveReplicaCount}/${app.replicas.length} healthy across ${app.replicaCounts.webservices} ws, ${app.replicaCounts.workers} workers, ${app.replicaCounts.wakers} wakers`;
    case "Secrets":
      return `${secrets.secrets.length} secrets`;
    case "Artifacts":
      return "File artifacts attached to runs";
    case "Notifications":
      return "In-app and email notifications";
    default:
      return resources.resources.find((resource) => resource.endpoint === resources.selectedResourceEndpoint)?.label ?? "";
  }
});
</script>
