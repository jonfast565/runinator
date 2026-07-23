<template>
  <div
    class="pointer-events-none fixed right-[18px] bottom-[18px] z-20 flex flex-col items-end gap-2"
    aria-live="polite"
  >
    <transition-group name="toast">
      <div
        v-for="toast in app.toasts"
        :key="toast.id"
        class="pointer-events-auto relative flex max-w-[min(520px,calc(100vw-40px))] items-center gap-2.5 overflow-hidden rounded-lg border px-3 py-2.5 shadow-toast"
        :class="toastToneClass(toast.kind)"
        role="status"
      >
        <span
          class="absolute inset-y-0 left-0 w-1"
          :class="toastAccentClass(toast.kind)"
          aria-hidden="true"
        ></span>
        <span
          class="inline-flex size-7 shrink-0 items-center justify-center rounded-md"
          :class="toastIconWellClass(toast.kind)"
        >
          <span
            v-if="toast.kind === 'loading'"
            class="toast-spinner size-[15px] animate-spin rounded-full border-2 border-current border-t-transparent opacity-90"
            aria-hidden="true"
          ></span>
          <Icon v-else :name="iconFor(toast.kind)" :size="15" />
        </span>
        <span class="min-w-0 flex-1 break-words pl-0.5 text-[13px] font-medium leading-snug">{{
          toast.text
        }}</span>
        <button
          class="inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-sm border-0 bg-transparent p-0 text-inherit opacity-60 transition-opacity duration-150 hover:opacity-100"
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

function toastToneClass(kind: ToastKind): string {
  switch (kind) {
    case "success":
      return "border-success-fg/35 bg-success-bg text-success-fg";
    case "error":
      return "border-danger-fg/40 bg-danger-bg text-danger-fg";
    case "loading":
      return "border-accent/35 bg-accent-soft text-accent-text";
    default:
      return "border-info-fg/35 bg-info-bg text-info-fg";
  }
}

function toastAccentClass(kind: ToastKind): string {
  switch (kind) {
    case "success":
      return "bg-success-fg";
    case "error":
      return "bg-danger";
    case "loading":
      return "bg-accent";
    default:
      return "bg-info-fg";
  }
}

function toastIconWellClass(kind: ToastKind): string {
  switch (kind) {
    case "success":
      return "bg-success-fg/15 text-success-fg";
    case "error":
      return "bg-danger/15 text-danger-fg";
    case "loading":
      return "bg-accent/15 text-accent-text";
    default:
      return "bg-info-fg/15 text-info-fg";
  }
}
</script>
