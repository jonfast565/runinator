<template>
  <aside class="sidebar" :class="{ collapsed: app.sidebarCollapsed }">
    <div class="brand" :class="railMode ? 'flex-col gap-2.5' : ''">
      <span class="brand-mark">R</span>
      <span class="brand-text" :class="railMode ? 'hidden' : ''">Command Center</span>
      <button
        class="sidebar-toggle inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md border-0 bg-transparent text-fg-inverse-muted hover:bg-inverse-hover hover:text-fg-inverse disabled:cursor-default"
        :class="railMode ? 'ml-0' : 'ml-auto'"
        :title="app.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :aria-label="app.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :disabled="app.interactionsDisabled"
        @click="app.toggleSidebar()"
      >
        <Icon
          :name="app.sidebarCollapsed ? 'chevron-right' : 'chevron-left'"
          :size="16"
          class="transition-transform duration-200 ease-out"
        />
      </button>
    </div>
    <nav class="nav-list">
      <template v-for="section in sections" :key="section.label">
        <div
          class="mt-2.5 px-2.5 text-[11px] font-semibold uppercase tracking-[0.04em] text-[#7e8c9c] first:mt-0"
          :class="railMode ? 'hidden' : ''"
        >
          {{ section.label }}
        </div>
        <button
          v-for="item in section.items"
          :key="item.tab"
          :class="{
            active: app.activeTab === item.tab,
            '!justify-center': railMode,
          }"
          :disabled="app.interactionsDisabled"
          :title="app.sidebarCollapsed ? item.label : undefined"
          @click="app.activeTab = item.tab"
        >
          <span class="inline-flex min-w-0 items-center" :class="railMode ? 'gap-0' : 'gap-[9px]'">
            <Icon :name="item.icon" :size="15" />
            <span
              class="overflow-hidden text-ellipsis whitespace-nowrap"
              :class="railMode ? 'hidden' : ''"
              >{{ item.label }}</span
            >
          </span>
          <span
            v-if="countFor(item.tab) !== null"
            class="nav-count"
            :class="railMode ? 'hidden' : ''"
            >{{ countFor(item.tab) }}</span
          >
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
import { useBreakpoint } from "../../composables/useBreakpoint";

const app = useAppStore();
const { isMobile } = useBreakpoint();
// desktop icon-rail only; the mobile drawer always shows labels regardless of the collapsed flag.
const railMode = computed(() => app.sidebarCollapsed && !isMobile.value);
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
