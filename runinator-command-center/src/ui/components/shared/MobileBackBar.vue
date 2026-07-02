<template>
  <button class="mobile-back-bar" type="button" @click="emit('back')">
    <Icon name="chevron-left" :size="16" />
    <span>{{ label }}</span>
  </button>
</template>

<script setup lang="ts">
import Icon from "./Icon.vue";

// back affordance for master-detail views on mobile; hidden on desktop where both panes are visible.
withDefaults(defineProps<{ label?: string }>(), { label: "Back to list" });
const emit = defineEmits<{ back: [] }>();
</script>

<style scoped>
.mobile-back-bar {
  display: none;
  align-items: center;
  gap: 4px;
  width: 100%;
  margin-bottom: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  color: var(--accent-text);
  font-size: 13px;
  font-weight: 600;
}

@media (max-width: 760px) {
  .mobile-back-bar {
    display: inline-flex;
  }
}
</style>
