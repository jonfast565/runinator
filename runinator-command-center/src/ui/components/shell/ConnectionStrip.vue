<template>
  <div ref="root" class="relative flex min-w-0 flex-1 flex-col items-end gap-1.5 overflow-hidden">
    <!-- on mobile the full summary collapses to one tappable status chip that reveals a popover. -->
    <button
      v-if="isMobile"
      class="inline-flex items-center gap-1.5 rounded-pill px-2.5 py-1 text-xs transition-[transform,filter] duration-150 ease-out active:scale-95"
      :class="chipToneClass"
      aria-label="Connection status"
      :aria-expanded="popoverOpen"
      @click="popoverOpen = !popoverOpen"
    >
      <span class="size-2 rounded-full bg-current"></span>
      <span>{{ connectionPillLabel }}</span>
    </button>
    <div
      v-show="!isMobile || popoverOpen"
      :class="
        isMobile
          ? 'ui-fade-up absolute top-[calc(100%+6px)] right-0 z-[45] flex w-max max-w-[78vw] origin-top-right flex-col flex-wrap items-start gap-1.5 overflow-visible rounded-lg border border-border bg-surface px-3 py-2.5 shadow-modal'
          : 'flex min-w-0 max-w-full flex-nowrap items-center justify-end gap-1.5 overflow-hidden'
      "
    >
      <span
        class="service-url min-w-0 flex-[0_1_auto] overflow-hidden text-ellipsis whitespace-nowrap"
        :class="isMobile ? 'max-w-full' : 'max-w-[clamp(120px,18vw,220px)]'"
        :title="app.serviceLabel"
        >{{ app.serviceLabel }}</span
      >
      <span v-if="!isMobile" class="connection-pill" :class="connectionPillClass">
        {{ connectionPillLabel }}
      </span>
      <span
        class="connection-pill inline-flex items-center gap-[5px] whitespace-nowrap"
        :class="streamStateClass"
      >
        <span v-if="app.eventStreamState === 'connected'" class="stream-state-dot"></span>
        {{ app.eventStreamLabel }}
      </span>
      <span v-if="!app.isRealtime && app.lastRefreshAt" class="last-refresh"
        >Last refresh: {{ app.lastRefreshText }}</span
      >
      <span
        v-if="app.hasReplicaState"
        class="overflow-hidden text-xs text-ellipsis whitespace-nowrap text-fg-muted"
      >
        {{ app.liveReplicaCount }}/{{ app.replicas.length }} healthy ·
        {{ app.replicaCounts.webservices }} ws · {{ app.replicaCounts.workers }} workers ·
        {{ app.replicaCounts.wakers }} wakers
      </span>
      <div
        v-if="supervisor.status.value?.configured"
        class="flex min-w-0 items-center gap-1 overflow-hidden"
      >
        <span
          v-for="proc in supervisor.status.value?.processes ?? []"
          :key="proc.name"
          class="shrink-0 rounded-[10px] border border-transparent px-[7px] py-px text-[10px] font-medium"
          :class="pillClass(proc.status, supervisor.status.value?.stale_seconds)"
          :title="processTooltip(proc)"
        >
          {{ proc.name }}
        </span>
        <span v-if="staleHint" class="ml-1 shrink-0 text-[10px] text-fg-faint">{{
          staleHint
        }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useBreakpoint } from "../../composables/useBreakpoint";
import { useSupervisorStatus } from "../../composables/useSupervisorStatus";
import type { SupervisorProcessSnapshot } from "../../../core/api/commandCenterApi";

const app = useAppStore();
const supervisor = useSupervisorStatus();
const { isMobile } = useBreakpoint();

const root = ref<HTMLElement | null>(null);
// popover is only visually active on mobile; track open state here for the chip toggle.
const popoverOpen = ref(false);

function onDocumentClick(event: MouseEvent) {
  if (popoverOpen.value && root.value && !root.value.contains(event.target as Node)) {
    popoverOpen.value = false;
  }
}

onMounted(() => {
  document.addEventListener("click", onDocumentClick);
});
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
});

// three-way tag: reachable (green), known-but-unreachable (red), or not yet discovered (amber).
const connectionPillClass = computed(() => {
  if (app.serviceConnected) {
    return "connected";
  }

  if (app.serviceKnown) {
    return "down";
  }

  return "waiting";
});
const chipToneClass = computed(() => {
  if (app.serviceConnected) {
    return "text-success-fg";
  }

  if (app.serviceKnown) {
    return "text-danger-fg";
  }

  return "text-warning-fg";
});
const connectionPillLabel = computed(() => {
  if (app.serviceConnected) {
    return "Service up";
  }

  if (app.serviceKnown) {
    return "Service down";
  }

  return "Service pending";
});

const streamStateClass = computed(() => {
  if (app.eventStreamState === "connected") {
    return "bg-success-bg text-success-fg";
  }

  if (app.eventStreamState === "connecting" || app.eventStreamState === "fallback") {
    return "bg-warning-bg text-warning-fg";
  }

  return "bg-surface-muted text-fg-muted";
});

const staleHint = computed(() => {
  const seconds = supervisor.status.value?.stale_seconds;

  if (seconds == null || seconds < 30) {
    return "";
  }

  return `state ${String(seconds)}s old`;
});

function pillClass(status: string, staleSeconds: number | null | undefined) {
  const stale = staleSeconds != null && staleSeconds > 30;

  if (stale) {
    return "bg-surface-muted text-fg-faint border-border-subtle";
  }

  const normalized = status.toLowerCase();

  if (normalized === "running") {
    return "bg-success-bg text-success-fg border-success-fg";
  }

  if (normalized === "starting" || normalized === "backoff") {
    return "bg-warning-bg text-warning-fg border-warning-fg";
  }

  if (normalized === "failed" || normalized === "exited" || normalized === "stopping") {
    return "bg-danger-bg text-danger-fg border-danger-fg";
  }

  return "bg-surface-muted text-fg-subtle border-border";
}

function processTooltip(proc: SupervisorProcessSnapshot): string {
  const parts: string[] = [];
  parts.push(`status: ${proc.status}`);

  if (proc.pid != null) {
    parts.push(`pid ${String(proc.pid)}`);
  }

  if (proc.uptime_seconds != null) {
    parts.push(`uptime ${formatUptime(proc.uptime_seconds)}`);
  }

  if (proc.restarts > 0) {
    parts.push(`${String(proc.restarts)} restarts`);
  }

  if (proc.last_exit_code != null) {
    parts.push(`last exit ${String(proc.last_exit_code)}`);
  }

  if (proc.last_error) {
    parts.push(proc.last_error);
  }

  return parts.join(" · ");
}

function formatUptime(seconds: number): string {
  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  const m = Math.floor(seconds / 60);

  if (m < 60) {
    return `${String(m)}m`;
  }

  const h = Math.floor(m / 60);
  return `${String(h)}h${String(m % 60)}m`;
}
</script>
