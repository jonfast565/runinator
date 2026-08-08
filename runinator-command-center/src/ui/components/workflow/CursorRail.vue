<template>
  <div v-if="markers.length" class="cursor-rail mb-2">
    <div class="mb-1 flex items-center justify-between">
      <span class="text-2xs uppercase tracking-wide text-muted">
        Threads of control ({{ markers.length }})
      </span>
      <label class="flex items-center gap-1 text-2xs text-muted">
        <input v-model="followCursor" type="checkbox" class="h-3 w-3" />
        <span>Follow</span>
      </label>
    </div>

    <ul class="flex flex-col gap-1">
      <li
        v-for="marker in markers"
        :key="marker.id"
        class="cursor-row"
        :class="{
          'cursor-row-selected': marker.selected,
          'cursor-row-speculative': marker.speculative,
        }"
        @click="select(marker.id)"
      >
        <span class="cursor-swatch" :style="{ background: paletteColor(marker.paletteIndex) }" />
        <span class="cursor-label" :title="marker.label">{{ marker.label }}</span>
        <span class="cursor-node" :title="marker.nodeId">{{ marker.nodeId }}</span>
        <span class="cursor-state" :class="marker.paused ? 'is-paused' : 'is-running'">
          {{ marker.paused ? "paused" : "running" }}
        </span>

        <span class="ml-auto flex items-center gap-1">
          <button
            class="btn btn-2xs"
            :disabled="!marker.paused"
            title="Step this branch one node"
            @click.stop="stepOne(marker.id)"
          >
            Step
          </button>
          <button
            class="btn btn-2xs"
            :disabled="!marker.paused"
            title="Continue this branch"
            @click.stop="continueOne(marker.id)"
          >
            Continue
          </button>
          <button
            v-if="marker.speculative"
            class="btn btn-2xs btn-danger"
            title="Abandon this speculative branch"
            @click.stop="retire(marker.id)"
          >
            Retire
          </button>
        </span>
      </li>
    </ul>

    <div class="mt-1 flex items-center gap-1">
      <button class="btn btn-2xs" title="Fork a speculative branch here" @click="openFork">
        Fork here
      </button>
      <span class="text-2xs text-muted">
        A forked branch shadows external effects unless a node is explicitly armed.
      </span>
    </div>

    <DebugJsonModal
      v-if="forkOpen"
      title="Fork a speculative branch"
      hint="Optional context patch, merged over this branch's context."
      :initial-value="{}"
      submit-label="Fork"
      @close="forkOpen = false"
      @submit="confirmFork"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import DebugJsonModal from "./DebugJsonModal.vue";

const workflows = useWorkflowsStore();

/**
 * follow the *selected* branch only. the run graph used to recentre on every change of the single
 * `current_node_id`; with several branches stepping independently that fires constantly, so the
 * camera is opt-in and scoped to one thread of control.
 */
const followCursor = computed({
  get: () => workflows.followCursor,
  set: (value: boolean) => workflows.setFollowCursor(value),
});

const markers = computed(() => workflows.cursorMarkers);
const forkOpen = ref(false);

// a fixed, colour-blind-safe rotation; index comes from the cursor's position in the persisted
// list, so a branch keeps its colour for as long as it is alive.
const PALETTE = [
  "#3b82f6",
  "#f59e0b",
  "#10b981",
  "#a855f7",
  "#ef4444",
  "#14b8a6",
  "#eab308",
  "#ec4899",
];

function paletteColor(index: number): string {
  return PALETTE[index % PALETTE.length] as string;
}

function select(cursorId: string) {
  workflows.selectCursor(cursorId);
}

function stepOne(cursorId: string) {
  workflows.selectCursor(cursorId);
  void workflows.stepSelectedWorkflowRun();
}

function continueOne(cursorId: string) {
  workflows.selectCursor(cursorId);
  void workflows.continueSelectedWorkflowRun();
}

function retire(cursorId: string) {
  void workflows.retireCursor(cursorId);
}

function openFork() {
  forkOpen.value = true;
}

function confirmFork(value: unknown) {
  forkOpen.value = false;
  void workflows.forkCursor({ contextPatch: value ?? null });
}
</script>

<style scoped>
.cursor-row {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.375rem;
  border-radius: 0.25rem;
  border: 1px solid transparent;
  cursor: pointer;
  font-size: 0.7rem;
}

.cursor-row:hover {
  background: var(--surface-hover, rgba(127, 127, 127, 0.12));
}

.cursor-row-selected {
  border-color: var(--accent, #f59e0b);
  background: var(--surface-hover, rgba(127, 127, 127, 0.14));
}

.cursor-row-speculative {
  border-style: dashed;
}

.cursor-swatch {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  flex: none;
}

.cursor-label {
  font-weight: 600;
  max-width: 9rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cursor-node {
  color: var(--text-muted, #9ca3af);
  max-width: 8rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cursor-state.is-paused {
  color: var(--accent, #f59e0b);
}

.cursor-state.is-running {
  color: var(--text-muted, #9ca3af);
}
</style>
