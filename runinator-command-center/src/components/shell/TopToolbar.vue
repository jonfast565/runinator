<template>
  <header class="topbar">
    <div class="view-title">
      <h1>{{ headingFor(app.activeTab) }}</h1>
      <span>{{ activeSubtitle }}</span>
    </div>
    <div class="toolbar-search">
      <input id="global-search" v-model="app.searchQuery" :disabled="app.serviceBlocked" placeholder="Search" />
    </div>
    <div class="actions">
      <ConnectionStrip />
      <button
        v-if="!app.isRealtime"
        class="btn"
        :disabled="app.serviceBlocked"
        @click="$emit('refresh')"
      >
        <Icon name="refresh" />
        <span>Refresh</span>
      </button>
      <button
        v-if="app.activeTab === 'Workflows'"
        class="btn btn-primary"
        :disabled="app.serviceBlocked || !workflows.canRunWorkflow"
        @click="workflows.runSelectedWorkflow()"
      >
        <Icon name="play" />
        <span>Run Workflow</span>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from "vue";
import Icon from "../shared/Icon.vue";
import ConnectionStrip from "./ConnectionStrip.vue";
import { navItemForTab, useAppStore } from "../../stores/app";
import { useResourcesStore } from "../../stores/resources";
import { useSecretsStore } from "../../stores/secrets";
import { useWorkflowsStore } from "../../stores/workflows";
import type { AppTab } from "../../types/app";

defineEmits<{ refresh: [] }>();

const app = useAppStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();

function headingFor(tab: AppTab): string {
  return navItemForTab(tab)?.label ?? String(tab);
}

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
