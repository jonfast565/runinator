<template>
  <div class="empty-state" :class="{ compact }">
    <div v-if="icon" class="empty-state-icon">
      <Icon :name="icon" :size="compact ? 18 : 24" />
    </div>
    <p class="empty-state-title">{{ title }}</p>
    <p v-if="description" class="empty-state-desc">{{ description }}</p>
    <div v-if="$slots.default" class="empty-state-actions">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import Icon, { type IconName } from "./Icon.vue";

// shared empty/first-run placeholder. keep messages actionable: say what is empty and what to do next.
withDefaults(
  defineProps<{
    title: string;
    description?: string;
    icon?: IconName;
    // compact = inline within a narrow panel (smaller, less padding); default is a centered block.
    compact?: boolean;
  }>(),
  { compact: false, description: undefined, icon: undefined },
);
</script>

<style scoped>
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  text-align: center;
  color: var(--text-muted);
  padding: 32px 20px;
}

.empty-state.compact {
  padding: 16px 12px;
  gap: 6px;
}

.empty-state-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  color: var(--text-subtle);
}

.empty-state.compact .empty-state-icon {
  width: 34px;
  height: 34px;
}

.empty-state-title {
  margin: 0;
  color: var(--text);
  font-weight: 600;
}

.empty-state.compact .empty-state-title {
  font-size: 13px;
}

.empty-state-desc {
  margin: 0;
  max-width: 42ch;
  font-size: 13px;
  line-height: 1.5;
}

.empty-state-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: center;
  margin-top: 6px;
}
</style>
