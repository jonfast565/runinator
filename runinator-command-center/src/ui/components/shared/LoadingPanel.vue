<template>
  <div class="loading-panel" :class="{ compact, inline }" role="status" aria-live="polite">
    <LoadingSpinner :size="compact || inline ? 'sm' : 'md'" :label="message" />
    <p v-if="message" class="loading-panel-message">{{ message }}</p>
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

<style scoped>
.loading-panel {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
  color: var(--text-muted);
  padding: 32px 20px;
  min-height: 120px;
}

.loading-panel.compact {
  padding: 16px 12px;
  min-height: 72px;
  gap: 8px;
}

.loading-panel.inline {
  flex-direction: row;
  justify-content: flex-start;
  align-items: center;
  min-height: 0;
  padding: 14px;
  gap: 10px;
}

.loading-panel-message {
  margin: 0;
  font-size: 13px;
  line-height: 1.4;
  max-width: 42ch;
}
</style>
