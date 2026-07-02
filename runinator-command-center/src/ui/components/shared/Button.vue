<template>
  <button
    :type="type"
    class="btn"
    :class="[
      variantClass,
      { 'btn-sm': size === 'sm', 'btn-icon': iconOnly, 'is-loading': loading },
    ]"
    :disabled="disabled || loading"
    @click="emit('click', $event)"
  >
    <span v-if="loading" class="btn-spinner" aria-hidden="true"></span>
    <Icon v-else-if="icon" :name="icon" :size="iconSize" />
    <span v-if="$slots.default" class="btn-label"><slot /></span>
  </button>
</template>

<script setup lang="ts">
import { computed } from "vue";
import Icon, { type IconName } from "./Icon.vue";

// shared button wrapping the .btn vocabulary in buttons.css. adds a built-in loading spinner
// (which also disables the button) so callers stop hand-rolling per-button busy state.
const props = withDefaults(
  defineProps<{
    variant?: "default" | "primary" | "danger" | "warn" | "ghost";
    size?: "md" | "sm";
    icon?: IconName;
    iconOnly?: boolean;
    loading?: boolean;
    disabled?: boolean;
    type?: "button" | "submit" | "reset";
  }>(),
  {
    variant: "default",
    size: "md",
    iconOnly: false,
    loading: false,
    disabled: false,
    type: "button",
    icon: undefined,
  },
);

const emit = defineEmits<{ click: [event: MouseEvent] }>();

const variantClass = computed(() => {
  switch (props.variant) {
    case "primary":
      return "btn-primary";
    case "danger":
      return "btn-danger";
    case "warn":
      return "btn-warn";
    case "ghost":
      return "btn-ghost";
    default:
      return "";
  }
});

const iconSize = computed(() => (props.size === "sm" ? 13 : 15));
</script>

<style scoped>
.btn-spinner {
  flex: 0 0 auto;
  width: 13px;
  height: 13px;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  opacity: 0.8;
  animation: btn-spin 0.7s linear infinite;
}

@keyframes btn-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
