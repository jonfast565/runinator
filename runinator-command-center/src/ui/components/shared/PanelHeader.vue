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
      <p v-if="description || $slots.description" class="m-0 text-xs text-fg-muted">
        <slot name="description">{{ description }}</slot>
      </p>
    </div>
    <div v-if="$slots.default" class="btn-row items-center self-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
// shared panel title row: title (+ optional subtitle) on the left, actions on the right.
// keeps multi-line headers start-aligned while action buttons stay vertically centered.
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
