<template>
  <div class="command-editor">
    <div v-if="optional" class="flex items-center justify-between gap-3">
      <div>
        <div class="text-sm font-medium">{{ label }}</div>
        <div v-if="description" class="mt-0.5 text-xs text-fg-muted">{{ description }}</div>
      </div>
      <button class="btn btn-sm" type="button" @click="toggle">
        <Icon :name="modelValue ? 'trash' : 'plus'" :size="13" />
        {{ modelValue ? "Remove" : "Add command" }}
      </button>
    </div>

    <template v-if="modelValue">
      <div v-if="!optional" class="mb-2">
        <div class="text-sm font-medium">{{ label }}</div>
        <div v-if="description" class="mt-0.5 text-xs text-fg-muted">{{ description }}</div>
      </div>
      <div class="grid gap-2">
        <label v-for="(_, index) in modelValue.argv" :key="index" class="field">
          <span>{{ index === 0 ? "Executable" : `Argument ${index}` }}</span>
          <div class="flex gap-2">
            <input
              class="input min-w-0 flex-1 font-mono"
              :aria-invalid="!modelValue.argv[index]?.trim()"
              :placeholder="index === 0 ? 'gh' : '--hostname'"
              :value="modelValue.argv[index]"
              @input="updateArg(index, ($event.target as HTMLInputElement).value)"
            />
            <button
              v-if="index > 0"
              class="btn btn-sm"
              type="button"
              :aria-label="`Remove argument ${index}`"
              @click="removeArg(index)"
            >
              <Icon name="trash" :size="13" />
            </button>
          </div>
        </label>
      </div>
      <div class="mt-2 flex flex-wrap items-center gap-3">
        <button class="btn btn-sm" type="button" @click="addArg">
          <Icon name="plus" :size="13" /> Add argument
        </button>
        <label v-if="allowInteractive" class="checkbox !mb-0">
          <input
            :checked="modelValue.interactive"
            type="checkbox"
            @change="updateInteractive(($event.target as HTMLInputElement).checked)"
          />
          Interactive desktop session
        </label>
      </div>
      <small v-if="error" class="field-error mt-2" role="alert">{{ error }}</small>
      <p class="mt-2 text-xs text-fg-muted">
        Arguments are executed directly as argv. Shell expansion, pipes, and redirects are not
        applied.
      </p>
    </template>
  </div>
</template>

<script setup lang="ts">
import type { ExecutionProfileCommand } from "../../../core/domain/models";
import Icon from "../shared/Icon.vue";

const props = withDefaults(
  defineProps<{
    modelValue: ExecutionProfileCommand | null | undefined;
    label: string;
    description?: string;
    optional?: boolean;
    allowInteractive?: boolean;
    error?: string;
  }>(),
  {
    allowInteractive: false,
    description: undefined,
    error: undefined,
    optional: false,
  },
);

const emit = defineEmits<{ "update:modelValue": [value: ExecutionProfileCommand | null] }>();

function toggle() {
  emit("update:modelValue", props.modelValue ? null : { argv: [""] });
}

function updateArg(index: number, value: string) {
  if (!props.modelValue) {
    return;
  }

  const argv = [...props.modelValue.argv];
  argv[index] = value;
  emit("update:modelValue", { ...props.modelValue, argv });
}

function addArg() {
  if (props.modelValue) {
    emit("update:modelValue", { ...props.modelValue, argv: [...props.modelValue.argv, ""] });
  }
}

function removeArg(index: number) {
  if (props.modelValue) {
    emit("update:modelValue", {
      ...props.modelValue,
      argv: props.modelValue.argv.filter((_, position) => position !== index),
    });
  }
}

function updateInteractive(interactive: boolean) {
  if (props.modelValue) {
    emit("update:modelValue", { ...props.modelValue, interactive });
  }
}
</script>

<style scoped>
.command-editor {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.75rem;
}
</style>
