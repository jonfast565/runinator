<template>
  <div
    class="flex w-full flex-col items-center justify-center text-center text-fg-muted"
    :class="compact ? 'gap-1.5 px-3 py-4' : 'gap-2 px-5 py-8'"
    role="status"
    :aria-live="loading ? 'polite' : undefined"
  >
    <LoadingSpinner v-if="loading" :size="compact ? 'sm' : 'md'" :label="loadingMessage || title" />
    <div
      v-else-if="icon"
      class="ui-fade-up inline-flex items-center justify-center rounded-pill border border-accent/20 bg-accent-soft text-accent-text"
      :class="compact ? 'size-[34px]' : 'size-11'"
    >
      <Icon :name="icon" :size="compact ? 18 : 24" />
    </div>
    <div
      class="m-0"
      :class="
        loading
          ? 'font-medium text-fg-muted'
          : compact
            ? 'text-[13px] font-semibold text-fg'
            : 'font-semibold text-fg'
      "
    >
      <span>{{ loading ? loadingMessage || title : title }}</span>
      <HelpBubble
        v-if="!loading && description"
        class="ml-0.5 align-middle"
        :text="description"
        label="About this empty state"
        :size="compact ? 14 : 16"
      />
    </div>
    <div v-if="!loading && $slots.default" class="mt-1.5 flex flex-wrap justify-center gap-2">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import HelpBubble from "./HelpBubble.vue";
import Icon, { type IconName } from "./Icon.vue";
import LoadingSpinner from "./LoadingSpinner.vue";

// Empty-state guidance stays available in help without adding instructional paragraphs to panes.
withDefaults(
  defineProps<{
    title: string;
    description?: string;
    icon?: IconName;
    loading?: boolean;
    loadingMessage?: string;
    // compact = inline within a narrow panel (smaller, less padding); default is a centered block.
    compact?: boolean;
  }>(),
  {
    compact: false,
    description: undefined,
    icon: undefined,
    loading: false,
    loadingMessage: undefined,
  },
);
</script>
