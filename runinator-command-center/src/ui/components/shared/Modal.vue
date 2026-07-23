<template>
  <div class="modal-backdrop" @click.self="onBackdrop">
    <div class="modal" :style="widthStyle" role="dialog" aria-modal="true">
      <div class="modal-header">
        <slot name="header">
          <h2>{{ title }}</h2>
        </slot>
        <button class="btn-close" aria-label="Close" @click="emit('close')">
          <Icon name="close" :size="16" />
        </button>
      </div>
      <div class="flex min-h-0 flex-col gap-3">
        <slot />
      </div>
      <div v-if="$slots.actions" class="modal-actions">
        <slot name="actions" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from "vue";
import Icon from "./Icon.vue";

// shared modal shell (styles live in tailwind.css @layer components). standardizes header +
// close button, footer actions, and escape/backdrop dismissal so every modal behaves the same.
const props = withDefaults(
  defineProps<{
    title?: string;
    // css width for the dialog, e.g. "560px" or "min(820px, 100%)".
    width?: string;
    closeOnBackdrop?: boolean;
    closeOnEsc?: boolean;
  }>(),
  { closeOnBackdrop: true, closeOnEsc: true, title: undefined, width: undefined },
);

const emit = defineEmits<{ close: [] }>();

const widthStyle = computed(() => (props.width ? { width: props.width } : undefined));

function onBackdrop() {
  if (props.closeOnBackdrop) {
    emit("close");
  }
}

function onKeydown(event: KeyboardEvent) {
  if (props.closeOnEsc && event.key === "Escape") {
    emit("close");
  }
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
});
</script>
