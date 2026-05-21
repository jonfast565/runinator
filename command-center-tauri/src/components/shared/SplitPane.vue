<template>
  <div ref="container" class="split-pane" :class="orientationClass" :style="splitStyle">
    <div class="split-section split-section-first">
      <slot name="first" />
    </div>
    <div class="split-handle" role="separator" :aria-orientation="separatorOrientation" tabindex="0" @pointerdown="startDrag" @keydown="onHandleKeydown" />
    <div class="split-section split-section-second">
      <slot name="second" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = withDefaults(
  defineProps<{
    orientation?: "horizontal" | "vertical";
    initialFirstPct?: number;
    minFirst?: number;
    minSecond?: number;
    storageKey?: string;
  }>(),
  {
    orientation: "horizontal",
    initialFirstPct: 50,
    minFirst: 260,
    minSecond: 320,
    storageKey: ""
  }
);

const container = ref<HTMLElement | null>(null);
const firstSize = ref(0);
let observer: ResizeObserver | undefined;

const orientationClass = computed(() => `split-pane-${props.orientation}`);
const separatorOrientation = computed(() => (props.orientation === "vertical" ? "horizontal" : "vertical"));
const splitStyle = computed(() => props.orientation === "vertical"
  ? { gridTemplateRows: `${firstSize.value}px 10px minmax(${props.minSecond}px, 1fr)` }
  : { gridTemplateColumns: `${firstSize.value}px 10px minmax(${props.minSecond}px, 1fr)` }
);

onMounted(() => {
  const saved = props.storageKey ? Number(window.localStorage.getItem(props.storageKey)) : 0;
  firstSize.value = saved > 0 ? saved : initialSize();
  observer = new ResizeObserver(() => {
    firstSize.value = clampSize(firstSize.value || initialSize());
  });
  if (container.value) observer.observe(container.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
});

function startDrag(event: PointerEvent) {
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", stopDrag);
}

function onPointerMove(event: PointerEvent) {
  const rect = container.value?.getBoundingClientRect();
  if (!rect) return;
  setFirstSize(props.orientation === "vertical" ? event.clientY - rect.top : event.clientX - rect.left);
}

function stopDrag() {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
}

function onHandleKeydown(event: KeyboardEvent) {
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
  if (props.storageKey) window.localStorage.setItem(props.storageKey, String(firstSize.value));
}

function initialSize(): number {
  const total = totalSize() || 1000;
  return clampSize((total * props.initialFirstPct) / 100);
}

function clampSize(size: number): number {
  const total = totalSize();
  if (total <= 0) return size;
  const max = Math.max(props.minFirst, total - props.minSecond - 10);
  return Math.min(max, Math.max(props.minFirst, size));
}

function totalSize(): number {
  if (!container.value) return 0;
  return props.orientation === "vertical" ? container.value.clientHeight : container.value.clientWidth;
}

function decrementKey(): string {
  return props.orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
}

function incrementKey(): string {
  return props.orientation === "vertical" ? "ArrowDown" : "ArrowRight";
}
</script>
