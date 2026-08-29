<template>
  <div class="panel-toolbar">
    <div class="flex min-w-0 items-center gap-1">
      <component
        :is="heading"
        class="m-0 font-semibold text-fg"
        :class="heading === 'h2' ? 'text-base' : 'text-sm'"
      >
        <slot name="title">{{ title }}</slot>
      </component>
      <HelpBubble v-if="description || $slots.help" :text="description" :label="helpLabel">
        <slot name="help">{{ description }}</slot>
      </HelpBubble>
    </div>
    <div v-if="$slots.default" class="btn-row items-center self-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import HelpBubble from "./HelpBubble.vue";

// Shared panel title row. Explanatory copy stays available without permanently occupying the pane.
withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    helpLabel?: string;
    heading?: "h2" | "h3";
  }>(),
  {
    title: undefined,
    description: undefined,
    helpLabel: "About this pane",
    heading: "h2",
  },
);
</script>
