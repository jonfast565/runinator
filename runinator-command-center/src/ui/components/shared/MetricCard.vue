<template>
  <div class="metric-card">
    <span class="text-xs text-fg-muted">{{ label }}</span>
    <strong class="truncate text-sm text-fg" :class="valueClass">
      <slot>{{ value }}</slot>
    </strong>
  </div>
</template>

<script setup lang="ts">
// compact summary tile used in list-panel dashboards (workflows, runs, secrets, …).
withDefaults(
  defineProps<{
    label: string;
    value?: string | number | null;
    valueClass?: string;
  }>(),
  {
    value: undefined,
    valueClass: undefined,
  },
);
</script>
