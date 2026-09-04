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
    <div class="view-identity">
      <div class="view-identity-icon" aria-hidden="true">
        <Icon :name="activeIcon" :size="20" />
      </div>
      <div class="view-title">
        <div class="view-title-heading">
          <span class="view-eyebrow">{{ activeSection }}</span>
          <span class="view-title-divider" aria-hidden="true">/</span>
          <h1>{{ headingFor(app.activeTab) }}</h1>
          <span v-if="app.loading && app.opLabel" class="view-status loading">
            <LoadingSpinner size="sm" :label="app.opLabel" />
            {{ app.opLabel }}…
          </span>
        </div>
      </div>
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
        v-if="showScopeSwitcher"
        class="org-select"
        :value="activeScopeValue"
        title="Active authorization scope"
        :disabled="app.interactionsDisabled || switchingScope"
        @change="onSwitchOrg"
      >
        <option v-if="hasPlatformAccess" :value="PLATFORM_SCOPE">Platform</option>
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
import { computed, ref } from "vue";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import ConnectionStrip from "./ConnectionStrip.vue";
import UserMenu from "./UserMenu.vue";
import { navItemForTab, useAppStore } from "../../../ui/adapters/pinia/app";
import { navSectionForTab } from "../../../core/navigation/nav-config";
import { useAuthStore } from "../../../ui/adapters/pinia/auth";
import { useOrgsStore } from "../../../ui/adapters/pinia/orgs";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useOperationLoading } from "../../composables/useOperationLoading";
import type { AppTab } from "../../../core/navigation/app";

defineEmits<{ refresh: [] }>();

const app = useAppStore();
const auth = useAuthStore();
const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const { isLoading: startingRun } = useOperationLoading("Running workflow", { prefix: true });
const PLATFORM_SCOPE = "__platform__";
const switchingScope = ref(false);

const hasPlatformAccess = computed(() => auth.user?.platform_role === "admin");
const showScopeSwitcher = computed(() => orgs.memberships.length > 1 || hasPlatformAccess.value);
const activeScopeValue = computed(
  () => orgs.activeOrgId ?? (hasPlatformAccess.value ? PLATFORM_SCOPE : ""),
);

async function onSwitchOrg(event: Event) {
  const orgId = (event.target as HTMLSelectElement).value;

  if (!orgId) {
    return;
  }

  switchingScope.value = true;

  try {
    if (orgId === PLATFORM_SCOPE) {
      await orgs.setActivePlatform();
    } else {
      await orgs.setActive(orgId);
    }
  } finally {
    switchingScope.value = false;
  }
}

function headingFor(tab: AppTab): string {
  return navItemForTab(tab)?.label ?? tab;
}

// only show the global search box on tabs whose list actually consumes app.searchQuery.
const searchPlaceholder = computed(() => navItemForTab(app.activeTab)?.searchPlaceholder ?? "");
const activeIcon = computed(() => navItemForTab(app.activeTab)?.icon ?? "info");
const activeSection = computed(() => navSectionForTab(app.activeTab));
</script>
