<template>
  <div ref="container" class="split-pane" :style="splitStyle">
    <div class="split-section split-section-first">
      <slot name="first" />
    </div>
    <div class="split-handle" role="separator" aria-orientation="vertical" tabindex="0" @pointerdown="startDrag" @keydown="onHandleKeydown" />
    <div class="split-section split-section-second">
      <slot name="second" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = withDefaults(
  defineProps<{
    initialFirstPct?: number;
    minFirst?: number;
    minSecond?: number;
    storageKey?: string;
  }>(),
  {
    initialFirstPct: 50,
    minFirst: 260,
    minSecond: 320,
    storageKey: ""
  }
);

const container = ref<HTMLElement | null>(null);
const firstWidth = ref(0);
let observer: ResizeObserver | undefined;

const splitStyle = computed(() => ({
  gridTemplateColumns: `${firstWidth.value}px 10px minmax(${props.minSecond}px, 1fr)`
}));

onMounted(() => {
  const saved = props.storageKey ? Number(window.localStorage.getItem(props.storageKey)) : 0;
  firstWidth.value = saved > 0 ? saved : initialWidth();
  observer = new ResizeObserver(() => {
    firstWidth.value = clampWidth(firstWidth.value || initialWidth());
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
  setFirstWidth(event.clientX - rect.left);
}

function stopDrag() {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
}

function onHandleKeydown(event: KeyboardEvent) {
  const step = event.shiftKey ? 60 : 20;
  if (event.key === "ArrowLeft") {
    event.preventDefault();
    setFirstWidth(firstWidth.value - step);
  }
  if (event.key === "ArrowRight") {
    event.preventDefault();
    setFirstWidth(firstWidth.value + step);
  }
}

function setFirstWidth(width: number) {
  firstWidth.value = clampWidth(width);
  if (props.storageKey) window.localStorage.setItem(props.storageKey, String(firstWidth.value));
}

function initialWidth(): number {
  const width = container.value?.clientWidth ?? 1000;
  return clampWidth((width * props.initialFirstPct) / 100);
}

function clampWidth(width: number): number {
  const total = container.value?.clientWidth ?? 0;
  if (total <= 0) return width;
  const max = Math.max(props.minFirst, total - props.minSecond - 10);
  return Math.min(max, Math.max(props.minFirst, width));
}
</script>
