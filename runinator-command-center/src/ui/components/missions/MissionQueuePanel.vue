<template>
  <section class="panel mission-queue-panel min-h-0 p-0">
    <PanelHeader
      class="px-3 pt-3"
      title="Mission queue"
      icon="runs"
      heading="h3"
      description="Filter durable mission bindings by their current operational state."
    >
      <Button size="sm" icon="refresh" :loading="loading" @click="emit('refresh')">Refresh</Button>
    </PanelHeader>

    <div class="mission-queue-tools">
      <label class="mission-search">
        <Icon name="search" :size="15" />
        <span class="sr-only">Find a mission</span>
        <input
          :value="search"
          placeholder="Find a mission"
          @input="emit('update:search', ($event.target as HTMLInputElement).value)"
        />
      </label>
      <div class="mission-filter-row" aria-label="Mission status filter">
        <button
          v-for="item in filters"
          :key="item.id"
          class="mission-filter"
          :class="{ 'is-selected': filter === item.id }"
          type="button"
          :aria-pressed="filter === item.id"
          @click="emit('update:filter', item.id)"
        >
          <span>{{ item.label }}</span>
          <span>{{ counts[item.id] }}</span>
        </button>
      </div>
    </div>

    <EmptyState
      v-if="loading && !missions.length && total === 0"
      compact
      loading
      title="Loading missions"
    />
    <EmptyState
      v-else-if="!missions.length && total === 0"
      compact
      icon="branch"
      title="No missions yet"
      description="Start a recipe to create a durable mission."
    />
    <EmptyState
      v-else-if="!missions.length"
      compact
      icon="search"
      title="No matching missions"
      description="Try a different search or status filter."
    >
      <Button size="sm" variant="ghost" @click="clearFilters">Clear filters</Button>
    </EmptyState>
    <div v-else class="mission-list" aria-label="Missions">
      <button
        v-for="mission in missions"
        :key="mission.id"
        class="mission-row"
        :class="{ 'is-selected': mission.id === selectedId }"
        type="button"
        @click="emit('select', mission.id)"
      >
        <span class="mission-state" :class="`is-${mission.status}`"></span>
        <span class="min-w-0 flex-1 text-left">
          <span class="mission-row-topline">
            <span class="block truncate font-medium text-fg">{{ mission.correlation_key }}</span>
            <span class="mission-status" :class="`is-${mission.status}`">{{
              statusLabel(mission.status)
            }}</span>
          </span>
          <span class="block truncate text-xs text-fg-muted">
            {{ phaseLabel(mission.current_phase) }} · {{ relativeDate(mission.updated_at) }}
          </span>
        </span>
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { OrchestrationBinding } from "../../../core/domain/models";
import Button from "../shared/Button.vue";
import EmptyState from "../shared/EmptyState.vue";
import Icon from "../shared/Icon.vue";
import PanelHeader from "../shared/PanelHeader.vue";

export type MissionQueueFilter = "all" | "active" | "attention" | "completed";

defineProps<{
  missions: OrchestrationBinding[];
  total: number;
  selectedId: string | null;
  loading: boolean;
  filter: MissionQueueFilter;
  search: string;
  counts: Record<MissionQueueFilter, number>;
}>();

const emit = defineEmits<{
  refresh: [];
  select: [id: string];
  "update:filter": [filter: MissionQueueFilter];
  "update:search": [search: string];
}>();

const filters: { id: MissionQueueFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "active", label: "Active" },
  { id: "attention", label: "Attention" },
  { id: "completed", label: "Complete" },
];

function clearFilters(): void {
  emit("update:filter", "all");
  emit("update:search", "");
}

function phaseLabel(phase: string | null | undefined): string {
  if (!phase) {
    return "Preparing to start";
  }

  return phase
    .replace(/^runinator\.missions\./, "")
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function statusLabel(status: string): string {
  return status === "completed" ? "Complete" : status.replaceAll("_", " ");
}

function relativeDate(value: string): string {
  const timestamp = new Date(value).getTime();

  if (Number.isNaN(timestamp)) {
    return value;
  }

  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));

  if (seconds < 60) {
    return "Updated now";
  }

  const minutes = Math.floor(seconds / 60);

  if (minutes < 60) {
    return `Updated ${String(minutes)}m ago`;
  }

  const hours = Math.floor(minutes / 60);

  if (hours < 24) {
    return `Updated ${String(hours)}h ago`;
  }

  return `Updated ${String(Math.floor(hours / 24))}d ago`;
}
</script>

<style scoped>
.mission-queue-panel {
  display: flex;
  flex-direction: column;
}
.mission-queue-tools {
  display: grid;
  gap: 0.55rem;
  border-bottom: 1px solid var(--color-border-subtle);
  padding: 0.75rem;
}
.mission-search {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.35rem 0.5rem;
  color: var(--color-fg-muted);
  background: var(--color-bg-subtle);
}
.mission-search:focus-within {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 16%, transparent);
}
.mission-search input {
  width: 100%;
  border: 0;
  outline: 0;
  color: var(--color-fg);
  background: transparent;
  font-size: 0.8rem;
}
.mission-filter-row {
  display: flex;
  gap: 0.25rem;
  overflow-x: auto;
  padding-bottom: 0.05rem;
}
.mission-filter {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 0.3rem;
  border: 1px solid transparent;
  border-radius: 999px;
  padding: 0.25rem 0.45rem;
  color: var(--color-fg-muted);
  background: transparent;
  font-size: 0.72rem;
}
.mission-filter span:last-child {
  min-width: 1rem;
  border-radius: 999px;
  padding: 0.05rem 0.25rem;
  color: inherit;
  background: var(--color-bg-subtle);
  text-align: center;
}
.mission-filter:hover,
.mission-filter.is-selected {
  border-color: color-mix(in srgb, var(--color-accent) 35%, transparent);
  color: var(--color-accent-text);
  background: var(--color-accent-soft);
}
.mission-list {
  min-height: 0;
  max-height: 55vh;
  overflow: auto;
  padding: 0.4rem;
}
.mission-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 0.6rem;
  border: 1px solid transparent;
  border-radius: 0.55rem;
  padding: 0.65rem;
  color: inherit;
  background: transparent;
}
.mission-row:hover,
.mission-row.is-selected {
  border-color: color-mix(in srgb, var(--color-accent) 35%, transparent);
  background: var(--color-accent-soft);
}
.mission-row-topline {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}
.mission-state {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 999px;
  background: var(--color-fg-muted);
  flex: none;
}
.mission-state.is-running,
.mission-state.is-pending {
  background: var(--color-accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 17%, transparent);
}
.mission-state.is-completed {
  background: var(--color-success-fg);
}
.mission-state.is-failed,
.mission-state.is-terminated {
  background: var(--color-danger-fg);
}
.mission-state.is-waiting,
.mission-state.is-suspended {
  background: var(--color-warning-fg);
}
.mission-status {
  flex: none;
  color: var(--color-fg-muted);
  font-size: 0.7rem;
  text-transform: capitalize;
}
.mission-status.is-running,
.mission-status.is-pending {
  color: var(--color-accent-text);
}
.mission-status.is-completed {
  color: var(--color-success-fg);
}
.mission-status.is-failed,
.mission-status.is-terminated {
  color: var(--color-danger-fg);
}
.mission-status.is-waiting,
.mission-status.is-suspended {
  color: var(--color-warning-fg);
}
@media (min-width: 1024px) {
  .mission-list {
    max-height: calc(100vh - 22rem);
  }
}
</style>
