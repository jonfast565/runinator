<template>
  <div class="rounded-md border border-border bg-surface-sunken px-2.5 py-2">
    <div class="mb-1 flex items-baseline justify-between gap-2">
      <span class="text-[11px] tracking-wide text-fg-muted uppercase">{{ label }}</span>
      <span class="font-mono text-xs font-semibold">{{ latestLabel }}</span>
    </div>
    <svg
      v-if="points.length > 1"
      class="block h-10 w-full"
      :viewBox="`0 0 ${String(width)} ${String(height)}`"
      preserveAspectRatio="none"
    >
      <polyline :points="areaPath" class="stroke-none" :style="{ fill: color, opacity: 0.12 }" />
      <polyline
        :points="linePath"
        class="fill-none [stroke-width:1.5] [vector-effect:non-scaling-stroke]"
        :style="{ stroke: color }"
      />
    </svg>
    <div v-else class="py-2.5 text-[11px] text-fg-muted">no samples yet</div>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";

// a dependency-free svg sparkline. no chart library is bundled in this app, so charts are hand-rolled.
const props = withDefaults(
  defineProps<{
    label: string;
    values: number[];
    color?: string;
    unit?: string;
    max?: number | null;
    format?: (value: number) => string;
  }>(),
  { color: "var(--accent)", unit: "", max: null, format: undefined },
);

const width = 200;
const height = 40;

const points = computed(() => props.values.filter((value) => Number.isFinite(value)));

// upper bound: an explicit max (e.g. 100 for percentages) or the observed peak with headroom.
const upper = computed(() => {
  if (props.max != null && props.max > 0) {
    return props.max;
  }

  const peak = Math.max(...points.value, 0);
  return peak > 0 ? peak * 1.1 : 1;
});

const coords = computed(() => {
  const list = points.value;

  if (list.length < 2) {
    return [] as { x: number; y: number }[];
  }

  const stepX = width / (list.length - 1);
  return list.map((value, index) => ({
    x: index * stepX,
    y: height - Math.min(1, Math.max(0, value / upper.value)) * height,
  }));
});

const linePath = computed(() =>
  coords.value.map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" "),
);

const areaPath = computed(() => {
  if (!coords.value.length) {
    return "";
  }

  const first = coords.value[0];
  const last = coords.value[coords.value.length - 1];
  return `${String(first.x)},${String(height)} ${linePath.value} ${String(last.x)},${String(height)}`;
});

const latestLabel = computed(() => {
  const list = points.value;

  if (!list.length) {
    return "—";
  }

  const latest = list[list.length - 1];

  if (props.format) {
    return props.format(latest);
  }

  return `${latest.toFixed(1)}${props.unit}`;
});
</script>
