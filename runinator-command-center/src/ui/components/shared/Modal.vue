<template>
  <div class="modal-backdrop" @click.self="onBackdrop">
    <div
      ref="dialog"
      class="modal"
      :style="widthStyle"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="title ? titleId : undefined"
      :aria-describedby="description ? descriptionId : undefined"
      tabindex="-1"
    >
      <div class="modal-header">
        <slot name="header">
          <div class="flex items-center gap-1">
            <h2 :id="titleId">{{ title }}</h2>
            <HelpBubble
              v-if="description || $slots.help"
              :text="description"
              label="About this window"
            >
              <slot name="help">{{ description }}</slot>
            </HelpBubble>
          </div>
        </slot>
        <button class="btn-close" aria-label="Close" @click="emit('close')">
          <Icon name="close" :size="16" />
        </button>
      </div>
      <p v-if="description" :id="descriptionId" class="sr-only">{{ description }}</p>
      <div class="modal-body">
        <slot />
      </div>
      <div v-if="$slots.actions" class="modal-actions">
        <slot name="actions" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId } from "vue";
import Icon from "./Icon.vue";
import HelpBubble from "./HelpBubble.vue";

// shared modal shell (styles live in tailwind.css @layer components). standardizes header +
// close button, footer actions, and escape/backdrop dismissal so every modal behaves the same.
const props = withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    // css width for the dialog, e.g. "560px" or "min(820px, 100%)".
    width?: string;
    closeOnBackdrop?: boolean;
    closeOnEsc?: boolean;
  }>(),
  {
    closeOnBackdrop: true,
    closeOnEsc: true,
    title: undefined,
    description: undefined,
    width: undefined,
  },
);

const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLElement | null>(null);
const id = useId();
const titleId = `${id}-title`;
const descriptionId = `${id}-description`;
const widthStyle = computed(() => (props.width ? { width: props.width } : undefined));
let previouslyFocused: HTMLElement | null = null;

function onBackdrop() {
  if (props.closeOnBackdrop) {
    emit("close");
  }
}

function onKeydown(event: KeyboardEvent) {
  if (props.closeOnEsc && event.key === "Escape") {
    emit("close");
    return;
  }

  if (event.key !== "Tab" || !dialog.value) {
    return;
  }

  const focusable = [...dialog.value.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
    (element) => !element.hasAttribute("disabled") && element.tabIndex !== -1,
  );

  if (!focusable.length) {
    event.preventDefault();
    dialog.value.focus();
    return;
  }

  const first = focusable[0];
  const last = focusable.at(-1) ?? first;

  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

onMounted(() => {
  previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  window.addEventListener("keydown", onKeydown);
  void nextTick(() => {
    const initialFocus = dialog.value?.querySelector<HTMLElement>("[autofocus]");
    (initialFocus ?? dialog.value)?.focus();
  });
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  previouslyFocused?.focus();
});

const FOCUSABLE_SELECTOR =
  'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';
</script>
