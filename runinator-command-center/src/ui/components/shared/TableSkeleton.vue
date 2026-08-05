<template>
  <div class="flex min-h-0 flex-1 flex-col gap-1.5 px-1 py-2" role="status" aria-live="polite">
    <span class="sr-only">{{ message }}</span>
    <div
      v-for="row in rows"
      :key="row"
      class="flex items-center gap-3"
      :style="{ animationDelay: `${String(row * 60)}ms` }"
      aria-hidden="true"
    >
      <span
        v-for="column in columns"
        :key="column"
        class="h-3 animate-pulse rounded-sm bg-fg/10"
        :style="{ width: widthFor(row, column) }"
      ></span>
    </div>
  </div>
</template>

<script setup lang="ts">
// content-shaped first-load placeholder for a table. unlike a centered spinner it holds the pane's
// height and column rhythm, so the real rows arrive in place instead of shifting the layout under
// the pointer. only for the first load — a background refresh dims the existing rows instead.
const props = withDefaults(
  defineProps<{
    rows?: number;
    columns?: number;
    message?: string;
  }>(),
  { rows: 6, columns: 4, message: "Loading…" },
);

// deterministic pseudo-random widths: a uniform grid reads as a broken table rather than as pending
// content, but real randomness would reflow on every re-render.
function widthFor(row: number, column: number): string {
  const spread = ((row * 7 + column * 13) % 5) * 8;
  const base = column === 1 ? 26 : column === props.columns ? 14 : 18;
  return `${String(base + spread)}%`;
}
</script>
