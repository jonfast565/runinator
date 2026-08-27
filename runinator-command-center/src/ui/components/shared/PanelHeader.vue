<template>
  <div class="panel-toolbar">
    <div class="grid min-w-0 gap-1">
      <component
        :is="heading"
        class="m-0 font-semibold text-fg"
        :class="heading === 'h2' ? 'text-base' : 'text-sm'"
      >
        <slot name="title">{{ title }}</slot>
      </component>
    </div>
    <div v-if="$slots.default" class="btn-row items-center self-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
// Shared panel title row. `description` remains accepted while callers are migrated, but panel
// chrome intentionally renders titles and controls only.
withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    heading?: "h2" | "h3";
  }>(),
  {
    title: undefined,
    description: undefined,
    heading: "h2",
  },
);
</script>
