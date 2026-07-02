<template>
  <transition name="outage">
    <div v-if="app.showOutageBanner" class="outage-banner" role="alert">
      <Icon name="alert" :size="16" class="outage-icon" />
      <span class="outage-text">
        Runinator service is unreachable right now. Retrying automatically&mdash;changes may not be
        saved until it reconnects.
      </span>
      <button class="outage-close" aria-label="Dismiss" @click="app.dismissOutageBanner()">
        <Icon name="close" :size="13" />
      </button>
    </div>
  </transition>
</template>

<script setup lang="ts">
import Icon from "../shared/Icon.vue";
import { useAppStore } from "../../stores/app";

const app = useAppStore();
</script>

<style scoped>
.outage-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--danger-fg);
  background: var(--danger-bg);
  color: var(--danger-fg);
  font-size: 13px;
}

.outage-icon {
  flex: 0 0 auto;
}

.outage-text {
  flex: 1 1 auto;
  min-width: 0;
}

.outage-close {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  opacity: 0.7;
  cursor: pointer;
}

.outage-close:hover {
  opacity: 1;
  background: transparent;
}

.outage-enter-active,
.outage-leave-active {
  transition:
    opacity 0.18s ease,
    transform 0.18s ease;
}

.outage-enter-from,
.outage-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}
</style>
