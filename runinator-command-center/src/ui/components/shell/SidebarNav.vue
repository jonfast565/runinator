<template>
  <aside
    ref="sidebarRef"
    class="sidebar"
    :class="{ collapsed: app.sidebarCollapsed, 'icon-rail': railMode }"
  >
    <div class="brand" :class="{ 'icon-rail-brand': railMode }">
      <BrandMark />
      <span v-if="!railMode" class="brand-text">Command Center</span>
      <button
        class="sidebar-toggle inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md border-0 bg-transparent text-fg-inverse-muted hover:bg-inverse-hover hover:text-fg-inverse disabled:cursor-default"
        :class="{ 'sidebar-toggle-rail': railMode, 'ml-auto': !railMode }"
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
    <nav class="nav-scroll">
      <div class="nav-list">
        <section
          v-for="section in sections"
          :key="section.label"
          class="nav-section"
          :class="{ 'nav-section-rail': railMode }"
          :aria-label="section.label"
        >
          <div v-if="!railMode" class="nav-section-label">
            {{ section.label }}
          </div>
          <button
            v-for="item in section.items"
            :key="item.tab"
            :class="{
              active: app.activeTab === item.tab,
              'icon-rail-item': railMode,
            }"
            :disabled="app.interactionsDisabled"
            :title="app.sidebarCollapsed ? item.label : undefined"
            :aria-label="railMode ? item.label : undefined"
            :aria-current="app.activeTab === item.tab ? 'page' : undefined"
            @click="app.activeTab = item.tab"
          >
            <span
              class="inline-flex min-w-0 items-center"
              :class="railMode ? 'gap-0' : 'gap-[9px]'"
            >
              <Icon :name="item.icon" :size="15" />
              <span v-if="!railMode" class="overflow-hidden text-ellipsis whitespace-nowrap">{{
                item.label
              }}</span>
            </span>
            <span v-if="countFor(item.tab) !== null && !railMode" class="nav-count">{{
              countFor(item.tab)
            }}</span>
          </button>
        </section>
      </div>
    </nav>
    <div v-if="!railMode" class="sidebar-foot">
      <div class="sidebar-clock" aria-hidden="true">
        <span class="sidebar-clock-label">Local</span>
        <span class="sidebar-clock-time">{{ localTime }}</span>
      </div>
      <div class="sidebar-clock" aria-hidden="true">
        <span class="sidebar-clock-label">UTC</span>
        <span class="sidebar-clock-time">{{ utcTime }}</span>
      </div>
      <div class="sidebar-build" :title="buildTitle">{{ buildLabel }}</div>
    </div>
    <div
      v-if="resizable"
      class="sidebar-resize"
      :class="{ 'is-dragging': sidebar.dragging.value }"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      tabindex="0"
      @pointerdown="sidebar.startDrag"
      @keydown="sidebar.onKeydown"
      @dblclick="sidebar.reset"
    ></div>
  </aside>
</template>

<script setup lang="ts">
import Icon from "../shared/Icon.vue";
import BrandMark from "./BrandMark.vue";
import { navSections, useAppStore } from "../../../ui/adapters/pinia/app";
import { useResourcesStore } from "../../../ui/adapters/pinia/resources";
import { useSecretsStore } from "../../../ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import type { AppTab } from "../../../core/navigation/app";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBreakpoint } from "../../composables/useBreakpoint";
import { useSidebarWidth } from "../../composables/useSidebarWidth";
import { buildTooltip, versionLabel } from "../../../core/utils/build-info";

const app = useAppStore();
const { isTablet, isMobile } = useBreakpoint();
// desktop icon-rail only; the mobile drawer always shows labels regardless of the collapsed flag.
const railMode = computed(() => app.sidebarCollapsed && !isMobile.value);
const sidebarRef = ref<HTMLElement | null>(null);
const sidebar = useSidebarWidth(sidebarRef);
// tablet and below pin the rail to a fixed width (or float it as a drawer), so there is nothing
// to drag there — the same rule SplitPane's handle follows.
const resizable = computed(() => !app.sidebarCollapsed && !isTablet.value);
const buildLabel = versionLabel();
const buildTitle = buildTooltip();
const sections = computed(() => app.visibleNavSections());
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();

const clockNow = ref(new Date());
let clockTimer: ReturnType<typeof setInterval> | undefined;

onMounted(() => {
  clockTimer = setInterval(() => {
    clockNow.value = new Date();
  }, 1000);
});

onBeforeUnmount(() => {
  clearInterval(clockTimer);
});

const localTime = computed(() => clockNow.value.toLocaleTimeString([], { hour12: false }));
// schedules, cron headers, and every persisted timestamp are utc, so the rail shows both.
const utcTime = computed(() =>
  clockNow.value.toLocaleTimeString([], { hour12: false, timeZone: "UTC" }),
);

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
