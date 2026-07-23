<template>
  <button
    v-if="isMobile"
    class="group mb-2 inline-flex w-full items-center gap-1 rounded-md border border-border bg-surface-subtle px-2.5 py-2 text-[13px] font-semibold text-accent-text transition-[background-color,border-color,transform] duration-150 ease-out hover:border-border-hover hover:bg-surface-hover active:scale-[0.99]"
    type="button"
    @click="emit('back')"
  >
    <Icon
      name="chevron-left"
      :size="16"
      class="transition-transform duration-150 ease-out group-hover:-translate-x-0.5"
    />
    <span>{{ label }}</span>
  </button>
</template>

<script setup lang="ts">
import Icon from "./Icon.vue";
import { useBreakpoint } from "../../composables/useBreakpoint";

// back affordance for master-detail views on mobile; hidden on desktop where both panes are visible.
withDefaults(defineProps<{ label?: string }>(), { label: "Back to list" });
const emit = defineEmits<{ back: [] }>();
const { isMobile } = useBreakpoint();
</script>
