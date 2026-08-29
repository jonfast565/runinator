<template>
  <span ref="root" class="help-bubble">
    <button
      type="button"
      class="help-bubble-trigger"
      :aria-label="label"
      :aria-expanded="open"
      :aria-controls="popoverId"
      @click="toggle"
      @keydown.esc.stop="close"
    >
      <Icon name="help" :size="size" />
    </button>
    <Teleport to="body">
      <div
        v-if="open"
        :id="popoverId"
        ref="popover"
        class="help-bubble-popover"
        role="tooltip"
        :style="position"
        @keydown.esc.stop="close"
      >
        <slot>{{ text }}</slot>
      </div>
    </Teleport>
  </span>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import Icon from "./Icon.vue";

let nextHelpBubbleId = 0;

withDefaults(
  defineProps<{
    text?: string;
    label?: string;
    size?: number;
  }>(),
  {
    text: undefined,
    label: "About this feature",
    size: 16,
  },
);

const root = ref<HTMLElement | null>(null);
const popover = ref<HTMLElement | null>(null);
const open = ref(false);
const position = ref<Record<string, string>>({});
nextHelpBubbleId += 1;
const popoverId = `help-bubble-${String(nextHelpBubbleId)}`;

function placePopover() {
  if (!root.value || !popover.value) {
    return;
  }

  const anchor = root.value.getBoundingClientRect();
  const bubble = popover.value.getBoundingClientRect();
  const gutter = 8;
  const left = Math.min(
    window.innerWidth - bubble.width - gutter,
    Math.max(gutter, anchor.left + anchor.width / 2 - bubble.width / 2),
  );
  const fitsBelow = anchor.bottom + gutter + bubble.height <= window.innerHeight - gutter;
  const top = fitsBelow
    ? anchor.bottom + gutter
    : Math.max(gutter, anchor.top - gutter - bubble.height);

  position.value = { left: `${String(left)}px`, top: `${String(top)}px` };
}

function toggle() {
  open.value = !open.value;

  if (open.value) {
    void nextTick(placePopover);
  }
}

function close() {
  open.value = false;
}

function onPointerDown(event: PointerEvent) {
  const target = event.target as Node;

  if (!root.value?.contains(target) && !popover.value?.contains(target)) {
    close();
  }
}

function onViewportChange() {
  if (open.value) {
    placePopover();
  }
}

onMounted(() => {
  document.addEventListener("pointerdown", onPointerDown);
  window.addEventListener("resize", onViewportChange);
  window.addEventListener("scroll", onViewportChange, true);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onPointerDown);
  window.removeEventListener("resize", onViewportChange);
  window.removeEventListener("scroll", onViewportChange, true);
});
</script>

<style scoped>
.help-bubble {
  display: inline-flex;
  flex: none;
  align-items: center;
}

.help-bubble-trigger {
  display: inline-flex;
  width: 1.65rem;
  height: 1.65rem;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--color-fg-muted);
  cursor: help;
}

.help-bubble-trigger:hover,
.help-bubble-trigger:focus-visible,
.help-bubble-trigger[aria-expanded="true"] {
  background: var(--color-surface-muted);
  color: var(--color-accent-text);
  outline: none;
}

.help-bubble-trigger:focus-visible {
  box-shadow: 0 0 0 2px var(--color-accent-soft);
}

.help-bubble-popover {
  position: fixed;
  z-index: 10000;
  width: min(20rem, calc(100vw - 1rem));
  padding: 0.7rem 0.8rem;
  border: 1px solid var(--color-border-strong);
  border-radius: 0.55rem;
  background: var(--color-surface);
  color: var(--color-fg);
  box-shadow: var(--shadow-modal);
  font-size: 0.78rem;
  font-weight: 400;
  line-height: 1.5;
  text-align: left;
  white-space: normal;
}
</style>
