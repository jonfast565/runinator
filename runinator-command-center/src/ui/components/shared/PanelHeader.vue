<template>
  <div class="panel-toolbar">
    <div class="panel-heading">
      <div v-if="icon" class="panel-heading-icon" aria-hidden="true">
        <Icon :name="icon" :size="heading === 'h2' ? 17 : 15" />
      </div>
      <div class="min-w-0">
        <span v-if="eyebrow" class="panel-eyebrow">{{ eyebrow }}</span>
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
      </div>
    </div>
    <div v-if="$slots.default" class="btn-row items-center self-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import HelpBubble from "./HelpBubble.vue";
import Icon, { type IconName } from "./Icon.vue";

// Shared panel title row. Explanatory copy stays available without permanently occupying the pane.
withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    helpLabel?: string;
    heading?: "h2" | "h3";
    icon?: IconName;
    eyebrow?: string;
  }>(),
  {
    title: undefined,
    description: undefined,
    helpLabel: "About this pane",
    heading: "h2",
    icon: undefined,
    eyebrow: undefined,
  },
);
</script>
