<template>
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-mark">R</span>
      <span>Command Center</span>
    </div>
    <nav class="nav-list">
      <template v-for="section in sections" :key="section.label">
        <div class="nav-section-label">{{ section.label }}</div>
        <button
          v-for="item in section.items"
          :key="item.tab"
          :class="{ active: app.activeTab === item.tab }"
          :disabled="app.serviceBlocked"
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
import { navSections, visibleNavSections, useAppStore } from "../../stores/app";
import { useResourcesStore } from "../../stores/resources";
import { useSecretsStore } from "../../stores/secrets";
import { useWorkflowsStore } from "../../stores/workflows";
import type { AppTab } from "../../types/app";

const app = useAppStore();
const sections = visibleNavSections();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();

function countFor(tab: AppTab): number | null {
  if (tab === "Runs") return workflows.recentWorkflowRuns.length;
  if (tab === "Workflows") return workflows.workflows.length;
  if (tab === "Replicas") return app.replicas.length;
  if (tab === "Secrets") return secrets.secrets.length;
  // Counts for resource tabs are only accurate for the currently-selected endpoint.
  if (resources.selectedResourceEndpoint === resourceEndpointFor(tab)) return resources.resourceRecords.length;
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
</style>
