<template>
  <div class="toast-stack" aria-live="polite">
    <transition-group name="toast">
      <div
        v-for="toast in app.toasts"
        :key="toast.id"
        class="toast"
        :class="`toast-${toast.kind}`"
        role="status"
      >
        <span v-if="toast.kind === 'loading'" class="toast-spinner" aria-hidden="true"></span>
        <Icon v-else :name="iconFor(toast.kind)" :size="16" class="toast-icon" />
        <span class="toast-text">{{ toast.text }}</span>
        <button class="toast-close" aria-label="Dismiss" @click="app.dismissToast(toast.id)">
          <Icon name="close" :size="13" />
        </button>
      </div>
    </transition-group>
  </div>
</template>

<script setup lang="ts">
import Icon, { type IconName } from "../shared/Icon.vue";
import { useAppStore, type ToastKind } from "../../stores/app";

const app = useAppStore();

function iconFor(kind: ToastKind): IconName {
  switch (kind) {
    case "success":
      return "check";
    case "error":
      return "alert";
    default:
      return "info";
  }
}
</script>

<style scoped>
.toast-stack {
  position: fixed;
  right: 18px;
  bottom: 18px;
  z-index: 20;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-end;
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: min(520px, calc(100vw - 40px));
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface);
  color: var(--text);
  padding: 10px 12px;
  box-shadow: var(--shadow-toast);
  pointer-events: auto;
}

.toast-icon {
  flex: 0 0 auto;
}

.toast-text {
  flex: 1 1 auto;
  min-width: 0;
  word-break: break-word;
}

/* neutral (info/loading) stays on the plain surface; only success/error get tonal color. */
.toast-success {
  border-color: var(--success-bg);
  background: var(--success-bg);
  color: var(--success-fg);
}

.toast-error {
  border-color: var(--danger-bg);
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.toast-info .toast-icon {
  color: var(--info-fg);
}

.toast-close {
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
  opacity: 0.6;
  cursor: pointer;
}

.toast-close:hover {
  opacity: 1;
  background: transparent;
}

.toast-spinner {
  flex: 0 0 auto;
  width: 15px;
  height: 15px;
  border: 2px solid var(--border-strong);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: toast-spin 0.7s linear infinite;
}

@keyframes toast-spin {
  to {
    transform: rotate(360deg);
  }
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 0.18s ease,
    transform 0.18s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
