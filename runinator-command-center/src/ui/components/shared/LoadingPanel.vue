<template>
  <div
    class="text-fg-muted"
    :class="
      inline
        ? 'flex min-h-0 flex-row items-center justify-start gap-2.5 p-3.5 text-left'
        : compact
          ? 'flex min-h-[72px] flex-col items-center justify-center gap-2 px-3 py-4 text-center'
          : 'flex min-h-[120px] flex-col items-center justify-center gap-2.5 px-5 py-8 text-center'
    "
    role="status"
    aria-live="polite"
  >
    <LoadingSpinner :size="compact || inline ? 'sm' : 'md'" :label="message" />
    <p v-if="message" class="m-0 max-w-[42ch] text-[13px] leading-snug">{{ message }}</p>
  </div>
</template>

<script setup lang="ts">
import LoadingSpinner from "./LoadingSpinner.vue";

// centered loading affordance for panels, tables, and list shells.
withDefaults(
  defineProps<{
    message?: string;
    compact?: boolean;
    inline?: boolean;
  }>(),
  {
    message: "Loading…",
    compact: false,
    inline: false,
  },
);
</script>
