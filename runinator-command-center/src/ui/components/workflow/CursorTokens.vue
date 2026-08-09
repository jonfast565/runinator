<template>
  <!-- teleported into Vue Flow's viewport, so the layer pans and zooms with the graph and each
       token's own transform only ever changes when its cursor moves. positioning these in screen
       space instead would re-run the travel transition on every pan frame. -->
  <Teleport v-if="viewport" :to="viewport">
    <div class="cursor-token-layer">
      <button
        v-for="token in tokens"
        :key="token.id"
        type="button"
        class="cursor-token"
        :class="{
          'is-selected': token.selected,
          'is-speculative': token.speculative,
          'is-paused': token.paused,
        }"
        :style="{ transform: `translate(-50%, -50%) translate(${token.x}px, ${token.y}px)` }"
        :title="`${token.label} — ${token.paused ? 'paused' : 'running'} at ${token.nodeId}`"
        :aria-label="`Thread of control ${token.label}, ${
          token.paused ? 'paused' : 'running'
        } at ${token.nodeId}`"
        @click.stop="select(token.id)"
      >
        <span class="cursor-token-hop" :class="hopClass(token.id)">
          <span class="cursor-token-halo" :style="{ background: color(token) }" />
          <span class="cursor-token-dot" :style="{ background: color(token) }" />
          <span class="cursor-token-land" :style="{ borderColor: color(token) }" />
        </span>
      </button>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useVueFlow } from "@vue-flow/core";
import { cursorColor } from "../../../core/domain/models";
import { advanceHopSequence, buildCursorTokens, type NodeBox } from "../../../core/workflow";
import type { CursorToken } from "../../../core/workflow";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";

const workflows = useWorkflowsStore();
const { getNodes, viewportRef } = useVueFlow();

const viewport = computed(() => viewportRef.value);

/** node boxes as the renderer has them, so a token lands on the node's measured centre. */
const boxes = computed(() => {
  const measured = new Map<string, NodeBox>();

  for (const node of getNodes.value) {
    measured.set(node.id, {
      x: node.position.x,
      y: node.position.y,
      width: node.dimensions.width,
      height: node.dimensions.height,
    });
  }

  return measured;
});

const tokens = computed(() => buildCursorTokens(workflows.cursorMarkers, boxes.value));

/**
 * a css animation cannot be replayed by re-setting the same name, so each jump alternates between
 * two identical keyframe sets. the counter's parity picks which, and flipping it is what makes the
 * next jump play. the class carries it rather than an inline `animation-name`, because scoped css
 * rewrites keyframe names and would leave an inline one pointing at nothing.
 */
const hops = ref(new Map<string, number>());
let previousNodes = new Map<string, string>();

watch(
  tokens,
  (next) => {
    const advanced = advanceHopSequence(next, previousNodes, hops.value);

    hops.value = advanced.sequence;
    previousNodes = advanced.positions;
  },
  { immediate: true },
);

function hopClass(cursorId: string): string {
  return (hops.value.get(cursorId) ?? 0) % 2 === 0 ? "is-hop-a" : "is-hop-b";
}

function color(token: CursorToken): string {
  return cursorColor(token.paletteIndex);
}

function select(cursorId: string) {
  workflows.selectCursor(cursorId);
}
</script>

<style scoped>
.cursor-token-layer {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  /* above the node cards, so a token crossing the graph is never hidden behind one. */
  z-index: 5;
}

.cursor-token {
  position: absolute;
  top: 0;
  left: 0;
  padding: 0;
  border: 0;
  background: none;
  line-height: 0;
  pointer-events: auto;
  cursor: pointer;
  /* the travel. only this element's transform changes when a cursor moves, and the easing lands
     hard rather than drifting in, which is what makes a step read as a discrete jump. */
  transition: transform 420ms cubic-bezier(0.22, 1, 0.36, 1);
}

/* the dot is half a rem, which is a fine *target* for a mouse and far too small for a thumb. on a
   touch pointer the button keeps its size and grows an invisible hit area around it. */
@media (pointer: coarse) {
  .cursor-token::before {
    content: "";
    position: absolute;
    inset: -1.1rem;
    border-radius: 9999px;
  }
}

.cursor-token-hop {
  position: relative;
  display: block;
  width: 0.5rem;
  height: 0.5rem;
  /* the arc, on its own element so the lift composes with the travel instead of fighting it for
     the same transform. */
  animation-duration: 420ms;
  animation-timing-function: ease-in-out;
}

.cursor-token-hop.is-hop-a {
  animation-name: cursor-hop-a;
}

.cursor-token-hop.is-hop-b {
  animation-name: cursor-hop-b;
}

@keyframes cursor-hop-a {
  0% {
    transform: translateY(0) scale(1);
  }
  45% {
    transform: translateY(-1.15rem) scale(1.3);
  }
  100% {
    transform: translateY(0) scale(1);
  }
}

/* identical to -a on purpose: alternating names is what restarts the animation. */
@keyframes cursor-hop-b {
  0% {
    transform: translateY(0) scale(1);
  }
  45% {
    transform: translateY(-1.15rem) scale(1.3);
  }
  100% {
    transform: translateY(0) scale(1);
  }
}

.cursor-token-dot {
  position: absolute;
  inset: 0;
  border-radius: 9999px;
  border: 1px solid var(--surface, #111827);
  box-shadow: 0 1px 3px rgb(0 0 0 / 45%);
}

/* the running halo: a branch the reducer is still driving breathes, a parked one is still. */
.cursor-token-halo {
  position: absolute;
  inset: -0.15rem;
  border-radius: 9999px;
  opacity: 0.35;
  animation: cursor-breathe 1.6s ease-in-out infinite;
}

.cursor-token.is-paused .cursor-token-halo {
  animation: none;
  opacity: 0;
}

@keyframes cursor-breathe {
  0%,
  100% {
    transform: scale(1);
    opacity: 0.35;
  }
  50% {
    transform: scale(1.7);
    opacity: 0;
  }
}

/* the landing ring: expands once as the token arrives. it runs on the same alternating class as
   the hop, so the ripple reads as that jump's impact rather than a second, unrelated pulse. */
.cursor-token-land {
  position: absolute;
  inset: -0.1rem;
  border-radius: 9999px;
  border: 1.5px solid transparent;
  opacity: 0;
  animation-duration: 420ms;
  animation-timing-function: ease-out;
}

.cursor-token-hop.is-hop-a .cursor-token-land {
  animation-name: cursor-land-a;
}

.cursor-token-hop.is-hop-b .cursor-token-land {
  animation-name: cursor-land-b;
}

@keyframes cursor-land-a {
  0%,
  60% {
    transform: scale(0.6);
    opacity: 0;
  }
  75% {
    opacity: 0.75;
  }
  100% {
    transform: scale(2.6);
    opacity: 0;
  }
}

@keyframes cursor-land-b {
  0%,
  60% {
    transform: scale(0.6);
    opacity: 0;
  }
  75% {
    opacity: 0.75;
  }
  100% {
    transform: scale(2.6);
    opacity: 0;
  }
}

.cursor-token.is-selected .cursor-token-dot {
  outline: 1.5px solid var(--accent, #f59e0b);
  outline-offset: 1.5px;
}

.cursor-token.is-speculative .cursor-token-dot {
  border-style: dashed;
  opacity: 0.9;
}

/* respect a reduced-motion preference: cursors still move, they just stop hopping and pulsing. */
@media (prefers-reduced-motion: reduce) {
  .cursor-token {
    transition: none;
  }

  .cursor-token-hop,
  .cursor-token-halo,
  .cursor-token-land {
    animation: none;
  }
}
</style>
