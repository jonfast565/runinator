<template>
  <div
    v-if="count > 0"
    class="ui-fade-up flex shrink-0 flex-wrap items-center gap-2 rounded-md border border-accent/25 bg-accent-soft px-2.5 py-2"
    role="region"
    :aria-label="`${String(count)} ${noun}${count === 1 ? '' : 's'} selected`"
  >
    <span class="text-[13px] font-semibold text-accent-text">
      {{ count }} {{ count === 1 ? noun : `${noun}s` }} selected
    </span>
    <div class="flex flex-1 flex-wrap items-center justify-end gap-2">
      <Button
        v-for="action in actions"
        :key="action.key"
        size="sm"
        :variant="action.variant ?? 'default'"
        :icon="action.icon"
        :disabled="busy !== '' || action.disabled"
        :loading="busy === action.key"
        @click="emit('run', action.key)"
      >
        {{ action.label }}
      </Button>
      <Button size="sm" variant="ghost" :disabled="busy !== ''" @click="emit('clear')">
        Clear
      </Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from "./Button.vue";
import type { IconName } from "./Icon.vue";

export interface BulkAction {
  key: string;
  label: string;
  icon?: IconName;
  variant?: "default" | "primary" | "danger" | "warn" | "ghost";
  disabled?: boolean;
}

// action bar shown above a table while rows are selected. purely presentational: the owning view
// holds the selection and performs the work, so this component never knows what a row is.
withDefaults(
  defineProps<{
    count: number;
    actions: BulkAction[];
    // the noun used in "N workflows selected"; pluralized by appending "s".
    noun: string;
    // key of the action currently running; disables the whole bar and spins that one button.
    busy?: string;
  }>(),
  { busy: "" },
);

const emit = defineEmits<{ run: [key: string]; clear: [] }>();
</script>
