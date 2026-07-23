<template>
  <span
    class="inline-block shrink-0 animate-spin rounded-full border-border-subtle border-t-accent motion-reduce:[animation-duration:1.6s]"
    :class="sizeClass"
    role="status"
    :aria-label="label"
    aria-live="polite"
  ></span>
</template>

<script setup lang="ts">
import { computed } from "vue";

// reusable inline spinner for panels, empty states, and non-button loading affordances.
const props = withDefaults(
  defineProps<{
    size?: "sm" | "md" | "lg";
    label?: string;
  }>(),
  {
    size: "md",
    label: "Loading",
  },
);

const sizeClass = computed(() => {
  switch (props.size) {
    case "sm":
      return "size-3.5 border-2";
    case "lg":
      return "size-8 border-[3px]";
    default:
      return "size-5 border-2";
  }
});
</script>
