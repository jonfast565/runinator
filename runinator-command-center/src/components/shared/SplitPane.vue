<template>
  <div
    ref="container"
    class="split-pane"
    :class="[orientationClass, { 'split-pane-collapsed': collapsedSide }]"
    :style="splitStyle"
  >
    <div class="split-section split-section-first">
      <slot name="first" />
    </div>
    <div
      class="split-handle"
      role="separator"
      :aria-orientation="separatorOrientation"
      tabindex="0"
      @pointerdown="startDrag"
      @keydown="onHandleKeydown"
    >
      <button
        v-if="collapsibleFirst"
        type="button"
        class="split-collapse-btn"
        :title="collapsedSide === 'first' ? 'Show panel' : 'Hide panel'"
        :aria-label="collapsedSide === 'first' ? 'Show panel' : 'Hide panel'"
        @pointerdown.stop.prevent
        @click="toggleCollapsed('first')"
      >
        <Icon :name="firstToggleIcon" :size="14" />
      </button>
      <button
        v-if="collapsibleSecond"
        type="button"
        class="split-collapse-btn"
        :title="collapsedSide === 'second' ? 'Show panel' : 'Hide panel'"
        :aria-label="collapsedSide === 'second' ? 'Show panel' : 'Hide panel'"
        @pointerdown.stop.prevent
        @click="toggleCollapsed('second')"
      >
        <Icon :name="secondToggleIcon" :size="14" />
      </button>
    </div>
    <div class="split-section split-section-second">
      <slot name="second" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import Icon from "./Icon.vue";

type CollapsedSide = "first" | "second" | "";

const props = withDefaults(
  defineProps<{
    orientation?: "horizontal" | "vertical";
    initialFirstPct?: number;
    minFirst?: number;
    minSecond?: number;
    storageKey?: string;
    collapsibleFirst?: boolean;
    collapsibleSecond?: boolean;
  }>(),
  {
    orientation: "horizontal",
    initialFirstPct: 50,
    minFirst: 260,
    minSecond: 320,
    storageKey: "",
    collapsibleFirst: false,
    collapsibleSecond: false,
  },
);

const container = ref<HTMLElement | null>(null);
const firstSize = ref(0);
const collapsedSide = ref<CollapsedSide>("");
let observer: ResizeObserver | undefined;

const orientationClass = computed(() => `split-pane-${props.orientation}`);
const separatorOrientation = computed(() =>
  props.orientation === "vertical" ? "horizontal" : "vertical",
);
// chevron points toward the pane it collapses; once hidden it points back to reveal it.
const firstToggleIcon = computed(() => {
  if (props.orientation === "vertical") {
    return collapsedSide.value === "first" ? "arrow-down" : "arrow-up";
  }

  return collapsedSide.value === "first" ? "chevron-right" : "chevron-left";
});
const secondToggleIcon = computed(() => {
  if (props.orientation === "vertical") {
    return collapsedSide.value === "second" ? "arrow-up" : "arrow-down";
  }

  return collapsedSide.value === "second" ? "chevron-left" : "chevron-right";
});
const collapsedKey = computed(() => (props.storageKey ? `${props.storageKey}::collapsed` : ""));
const splitStyle = computed(() => {
  const dimension = props.orientation === "vertical" ? "gridTemplateRows" : "gridTemplateColumns";
  let tracks: string;

  if (collapsedSide.value === "first") {
    tracks = `0px 10px minmax(0, 1fr)`;
  } else if (collapsedSide.value === "second") {
    tracks = `minmax(0, 1fr) 10px 0px`;
  } else {
    tracks = `${String(firstSize.value)}px 10px minmax(${String(props.minSecond)}px, 1fr)`;
  }

  return { [dimension]: tracks };
});

function toggleCollapsed(side: "first" | "second") {
  collapsedSide.value = collapsedSide.value === side ? "" : side;

  if (collapsedKey.value) {
    window.localStorage.setItem(collapsedKey.value, collapsedSide.value);
  }
}

onMounted(() => {
  const savedSide = collapsedKey.value ? window.localStorage.getItem(collapsedKey.value) : null;

  if (savedSide === "first" && props.collapsibleFirst) {
    collapsedSide.value = "first";
  } else if (savedSide === "second" && props.collapsibleSecond) {
    collapsedSide.value = "second";
  }

  const saved = props.storageKey ? Number(window.localStorage.getItem(props.storageKey)) : 0;
  firstSize.value = saved > 0 ? saved : initialSize();
  observer = new ResizeObserver(() => {
    firstSize.value = clampSize(firstSize.value || initialSize());
  });

  if (container.value) {
    observer.observe(container.value);
  }
});

onBeforeUnmount(() => {
  observer?.disconnect();
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
});

function startDrag(event: PointerEvent) {
  if (collapsedSide.value) {
    return;
  }

  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", stopDrag);
}

function onPointerMove(event: PointerEvent) {
  const rect = container.value?.getBoundingClientRect();

  if (!rect) {
    return;
  }

  setFirstSize(
    props.orientation === "vertical" ? event.clientY - rect.top : event.clientX - rect.left,
  );
}

function stopDrag() {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
}

function onHandleKeydown(event: KeyboardEvent) {
  if (collapsedSide.value) {
    return;
  }

  const step = event.shiftKey ? 60 : 20;

  if (event.key === decrementKey()) {
    event.preventDefault();
    setFirstSize(firstSize.value - step);
  }

  if (event.key === incrementKey()) {
    event.preventDefault();
    setFirstSize(firstSize.value + step);
  }
}

function setFirstSize(size: number) {
  firstSize.value = clampSize(size);

  if (props.storageKey) {
    window.localStorage.setItem(props.storageKey, String(firstSize.value));
  }
}

function initialSize(): number {
  const total = totalSize() || 1000;
  return clampSize((total * props.initialFirstPct) / 100);
}

function clampSize(size: number): number {
  const total = totalSize();

  if (total <= 0) {
    return size;
  }

  const max = Math.max(props.minFirst, total - props.minSecond - 10);
  return Math.min(max, Math.max(props.minFirst, size));
}

function totalSize(): number {
  if (!container.value) {
    return 0;
  }

  return props.orientation === "vertical"
    ? container.value.clientHeight
    : container.value.clientWidth;
}

function decrementKey(): string {
  return props.orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
}

function incrementKey(): string {
  return props.orientation === "vertical" ? "ArrowDown" : "ArrowRight";
}
</script>
