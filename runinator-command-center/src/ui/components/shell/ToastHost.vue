<template>
  <div
    class="pointer-events-none fixed right-[18px] bottom-[18px] z-20 flex flex-col items-end gap-2"
    aria-live="polite"
  >
    <transition-group name="toast">
      <div
        v-for="toast in app.toasts"
        :key="toast.id"
        class="pointer-events-auto flex max-w-[min(520px,calc(100vw-40px))] items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2.5 text-fg shadow-toast"
        :class="{
          'border-success-bg bg-success-bg text-success-fg': toast.kind === 'success',
          'border-danger-bg bg-danger-bg text-danger-fg': toast.kind === 'error',
        }"
        role="status"
      >
        <span
          v-if="toast.kind === 'loading'"
          class="toast-spinner size-[15px] shrink-0 animate-spin rounded-full border-2 border-border-strong border-t-accent"
          aria-hidden="true"
        ></span>
        <Icon
          v-else
          :name="iconFor(toast.kind)"
          :size="16"
          class="shrink-0"
          :class="toast.kind === 'info' ? 'text-info-fg' : ''"
        />
        <span class="min-w-0 flex-1 break-words">{{ toast.text }}</span>
        <button
          class="inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-sm border-0 bg-transparent p-0 text-inherit opacity-60 hover:opacity-100"
          aria-label="Dismiss"
          @click="app.dismissToast(toast.id)"
        >
          <Icon name="close" :size="13" />
        </button>
      </div>
    </transition-group>
  </div>
</template>

<script setup lang="ts">
import Icon, { type IconName } from "../shared/Icon.vue";
import { useAppStore, type ToastKind } from "../../../ui/adapters/pinia/app";

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
