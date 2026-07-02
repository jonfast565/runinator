<template>
  <aside class="sidebar" :class="{ collapsed: app.sidebarCollapsed }">
    <div class="brand">
      <span class="brand-mark">R</span>
      <span class="brand-text">Command Center</span>
      <button
        class="sidebar-toggle"
        :title="app.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :aria-label="app.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :disabled="app.interactionsDisabled"
        @click="app.toggleSidebar()"
      >
        <Icon :name="app.sidebarCollapsed ? 'chevron-right' : 'chevron-left'" :size="16" />
      </button>
    </div>
    <nav class="nav-list">
      <template v-for="section in sections" :key="section.label">
        <div class="nav-section-label">{{ section.label }}</div>
        <button
          v-for="item in section.items"
          :key="item.tab"
          :class="{ active: app.activeTab === item.tab }"
          :disabled="app.interactionsDisabled"
          :title="app.sidebarCollapsed ? item.label : undefined"
          @click="app.activeTab = item.tab"
        >
          <span class="nav-row">
            <Icon :name="item.icon" :size="15" />
            <span class="nav-label">{{ item.label }}</span>
          </span>
          <span v-if="countFor(item.tab) !== null" class="nav-count">{{ countFor(item.tab) }}</span>
        </button>
      </template>
    </nav>
  </aside>
</template>

<script setup lang="ts">
import Icon from "../shared/Icon.vue";
import { navSections, useAppStore } from "../../../ui/adapters/pinia/app";
import { useResourcesStore } from "../../../ui/adapters/pinia/resources";
import { useSecretsStore } from "../../../ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import type { AppTab } from "../../../core/navigation/app";
import { computed } from "vue";

const app = useAppStore();
const sections = computed(() => app.visibleNavSections());
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();

function countFor(tab: AppTab): number | null {
  if (tab === "Runs") {
    return workflows.recentWorkflowRuns.length;
  }

  if (tab === "Workflows") {
    return workflows.workflows.length;
  }

  if (tab === "Replicas") {
    return app.replicas.length;
  }

  if (tab === "Secrets") {
    return secrets.secrets.length;
  }

  // Counts for resource tabs are only accurate for the currently-selected endpoint.
  if (resources.selectedResourceEndpoint === resourceEndpointFor(tab)) {
    return resources.resourceRecords.length;
  }

  return null;
}

function resourceEndpointFor(tab: AppTab): string | undefined {
  const item = navSections.flatMap((section) => section.items).find((entry) => entry.tab === tab);
  return item?.endpoint;
}
</script>

<style scoped>
.nav-section-label {
  color: #7e8c9c;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  margin-top: 10px;
  padding: 0 10px;
  text-transform: uppercase;
}

.nav-section-label:first-child {
  margin-top: 0;
}

.nav-row {
  display: inline-flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

.nav-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidebar-toggle {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-left: auto;
  width: 26px;
  height: 26px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-inverse-muted);
  cursor: pointer;
}

.sidebar-toggle:hover {
  background: var(--surface-inverse-hover);
  color: var(--text-inverse);
}

/* collapsed: icon rail only. */
.sidebar.collapsed .brand-text,
.sidebar.collapsed .nav-section-label,
.sidebar.collapsed .nav-label,
.sidebar.collapsed .nav-count {
  display: none;
}

.sidebar.collapsed .brand {
  flex-direction: column;
  gap: 10px;
}

.sidebar.collapsed .sidebar-toggle {
  margin-left: 0;
}

.sidebar.collapsed :deep(.nav-list button) {
  justify-content: center;
}

.sidebar.collapsed .nav-row {
  gap: 0;
}

/* in the mobile drawer the sidebar is full-width regardless of the persisted collapsed flag:
   always show labels, counts, and section headers so nav is usable. */
@media (max-width: 760px) {
  .sidebar.collapsed .brand-text,
  .sidebar.collapsed .nav-section-label,
  .sidebar.collapsed .nav-label,
  .sidebar.collapsed .nav-count {
    display: revert;
  }

  .sidebar.collapsed .brand {
    flex-direction: row;
  }

  .sidebar.collapsed :deep(.nav-list button) {
    justify-content: space-between;
  }

  .sidebar.collapsed .nav-row {
    gap: 9px;
  }
}
</style>
