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
        <span class="cursor-swatch" :style="{ background: cursorColor(marker.paletteIndex) }" />
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
            class="btn btn-2xs"
            :class="{ 'is-armed': marker.armed }"
            :title="
              marker.armed
                ? `Disarm ${marker.nodeId}: shadow it again instead of dispatching for real`
                : `Arm ${marker.nodeId}: let this branch dispatch it for real, once`
            "
            @click.stop="toggleArm(marker)"
          >
            {{ marker.armed ? "Armed" : "Arm" }}
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
      <button
        v-if="markers.length > 1"
        class="btn btn-2xs"
        :title="compareWith ? 'Stop comparing' : 'Compare two branches'"
        @click="toggleCompare"
      >
        {{ compareWith ? "Stop compare" : "Compare" }}
      </button>
      <span class="text-2xs text-muted">
        A forked branch shadows external effects unless a node is explicitly armed.
      </span>
    </div>

    <!-- two branches side by side: the same diff the debugger already uses for input vs output,
         pointed at two threads of control instead of two moments of one. -->
    <JsonDiff
      v-if="comparison"
      :before="comparison.before"
      :after="comparison.after"
      :title="comparison.title"
      open
    />

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
import { cursorColor, type CursorMarker } from "../../../core/domain/models";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import DebugJsonModal from "./DebugJsonModal.vue";
import JsonDiff from "./JsonDiff.vue";

const workflows = useWorkflowsStore();

/**
 * follow the *selected* branch only. the run graph used to recentre on every change of the single
 * `current_node_id`; with several branches stepping independently that fires constantly, so the
 * camera is opt-in and scoped to one thread of control.
 */
const followCursor = computed({
  get: () => workflows.followCursor,
  set: (value: boolean) => { workflows.setFollowCursor(value); },
});

const markers = computed(() => workflows.cursorMarkers);
const forkOpen = ref(false);
/** the branch being compared *against* the selected one, if the operator asked for a comparison. */
const compareWith = ref<string>("");

/** pick the next branch that is not the selected one, so one click gives a useful pair. */
function toggleCompare() {
  if (compareWith.value) {
    compareWith.value = "";
    return;
  }

  const other = markers.value.find((marker) => !marker.selected);

  compareWith.value = other?.id ?? "";
}

const comparison = computed(() => {
  if (!compareWith.value) {
    return null;
  }

  const cursors = workflows.cursors;
  const selectedId = workflows.selectedCursorId;
  const left = cursors.find((cursor) => cursor.id === selectedId);
  const right = cursors.find((cursor) => cursor.id === compareWith.value);

  if (!left || !right) {
    return null;
  }

  const name = (id: string) => markers.value.find((marker) => marker.id === id)?.label ?? id;

  return {
    before: left.debug?.last_output_json ?? left.last_output ?? null,
    after: right.debug?.last_output_json ?? right.last_output ?? null,
    title: `${name(left.id)} vs ${name(right.id)}`,
  };
});

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

/**
 * arm or disarm the node this speculative branch is standing on. arming is per node, so the rail
 * offers it only for the branch's current position -- the one node it is about to execute.
 */
function toggleArm(marker: CursorMarker) {
  void workflows.armNodeForReal(marker.id, marker.nodeId, !marker.armed);
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

/* an armed node is the one place a "what if" branch reaches the outside world; make it read as a
   live state rather than another idle button. */
.btn.is-armed {
  border-color: var(--danger, #ef4444);
  color: var(--danger, #ef4444);
}

.cursor-state.is-paused {
  color: var(--accent, #f59e0b);
}

.cursor-state.is-running {
  color: var(--text-muted, #9ca3af);
}
</style>
