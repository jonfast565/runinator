<template>
  <span
    class="loading-spinner"
    :class="[`size-${size}`]"
    role="status"
    :aria-label="label"
    aria-live="polite"
  ></span>
</template>

<script setup lang="ts">
// reusable inline spinner for panels, empty states, and non-button loading affordances.
withDefaults(
  defineProps<{
    size?: "sm" | "md" | "lg";
    label?: string;
  }>(),
  {
    size: "md",
    label: "Loading",
  },
);
</script>

<style scoped>
.loading-spinner {
  display: inline-block;
  flex: 0 0 auto;
  border: 2px solid var(--border-subtle);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: loading-spin 0.8s linear infinite;
}

.size-sm {
  width: 14px;
  height: 14px;
}

.size-md {
  width: 20px;
  height: 20px;
  border-width: 2px;
}

.size-lg {
  width: 32px;
  height: 32px;
  border-width: 3px;
}

@keyframes loading-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .loading-spinner {
    animation-duration: 1.6s;
  }
}
</style>
