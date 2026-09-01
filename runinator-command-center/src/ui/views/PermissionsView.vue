<template>
  <section class="pane h-full overflow-hidden">
    <div
      class="panel grid h-full min-h-0 gap-3 grid-rows-[auto_auto_1fr] max-[920px]:overflow-auto"
    >
      <PanelHeader
        title="Permissions"
        icon="shield"
        eyebrow="Access control"
        description="Manage platform users, teams, workflow grants, and scoped API keys."
      >
        <button class="btn" :disabled="loading" @click="refresh">
          <LoadingSpinner v-if="loading" size="sm" label="Refreshing permissions" />
          <Icon v-else name="refresh" />
          <span>Refresh</span>
        </button>
      </PanelHeader>

      <nav
        class="inline-flex w-fit overflow-hidden rounded-md border border-border max-[920px]:max-w-full max-[920px]:overflow-x-auto"
        aria-label="Permissions sections"
      >
        <button
          v-for="tab in tabs"
          :key="tab.id"
          class="border-0 border-r border-border bg-surface px-3 py-1.5 text-fg-muted last:border-r-0"
          :class="activeTab === tab.id ? 'bg-accent-soft font-semibold text-fg' : ''"
          type="button"
          @click="activeTab = tab.id"
        >
          {{ tab.label }}
        </button>
      </nav>

      <UsersPermissionsPanel v-if="activeTab === 'users'" />
      <TeamsPermissionsPanel v-else-if="activeTab === 'teams'" />
      <WorkflowAccessPanel v-else-if="activeTab === 'access'" />
      <ApiKeysPermissionsPanel v-else />
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { usePermissionsStore } from "../adapters/pinia/permissions";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import ApiKeysPermissionsPanel from "../components/permissions/ApiKeysPermissionsPanel.vue";
import TeamsPermissionsPanel from "../components/permissions/TeamsPermissionsPanel.vue";
import UsersPermissionsPanel from "../components/permissions/UsersPermissionsPanel.vue";
import WorkflowAccessPanel from "../components/permissions/WorkflowAccessPanel.vue";
import Icon from "../components/shared/Icon.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import { useOperationLoading } from "../composables/useOperationLoading";

type PermissionsTab = "users" | "teams" | "access" | "apiKeys";

const tabs: { id: PermissionsTab; label: string }[] = [
  { id: "users", label: "Users" },
  { id: "teams", label: "Teams" },
  { id: "access", label: "Access" },
  { id: "apiKeys", label: "API Keys" },
];

const permissions = usePermissionsStore();
const workflows = useWorkflowsStore();
const { isLoading: loading } = useOperationLoading([
  "Loading permissions",
  "Loading API keys",
  "Loading workflow access",
]);
const activeTab = ref<PermissionsTab>("users");

async function refresh() {
  await Promise.all([
    permissions.refreshAll(),
    workflows.workflows.length === 0 ? workflows.refreshWorkflows() : Promise.resolve(),
  ]);
}

onMounted(refresh);
</script>
