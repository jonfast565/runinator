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
      <div class="mt-3 border-t border-border-subtle pt-3">
        <div class="flex items-center justify-between gap-3">
          <div>
            <div class="text-xs font-medium">Command environment</div>
            <div class="mt-0.5 text-xs text-fg-muted">
              A leading <code>~/</code> is expanded on the collecting desktop.
            </div>
          </div>
          <button class="btn btn-sm" type="button" @click="addEnvironment">
            <Icon name="plus" :size="13" /> Add variable
          </button>
        </div>
        <div
          v-for="([name, value], index) in environmentEntries()"
          :key="`${name}-${index}`"
          class="mt-2 grid gap-2 sm:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)_auto]"
        >
          <input
            class="input min-w-0 font-mono"
            :aria-invalid="!portableEnvironmentName(name)"
            aria-label="Environment variable name"
            placeholder="PROVIDER_HOME"
            :value="name"
            @input="updateEnvironmentName(index, ($event.target as HTMLInputElement).value)"
          />
          <input
            class="input min-w-0 font-mono"
            aria-label="Environment variable value"
            placeholder="~/.runinator/execution-profiles/provider"
            :value="value"
            @input="updateEnvironmentValue(index, ($event.target as HTMLInputElement).value)"
          />
          <button
            class="btn btn-sm"
            type="button"
            aria-label="Remove environment variable"
            @click="removeEnvironment(index)"
          >
            <Icon name="trash" :size="13" />
          </button>
        </div>
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
  if (!props.modelValue) {
    return;
  }

  emit("update:modelValue", {
    ...props.modelValue,
    argv: props.modelValue.argv.filter((_, position) => position !== index),
  });
}

function updateInteractive(interactive: boolean) {
  if (props.modelValue) {
    emit("update:modelValue", { ...props.modelValue, interactive });
  }
}

function environmentEntries(): [string, string][] {
  return Object.entries(props.modelValue?.environment ?? {});
}

function emitEnvironment(entries: [string, string][]) {
  if (!props.modelValue) {
    return;
  }

  emit("update:modelValue", {
    ...props.modelValue,
    environment: Object.fromEntries(entries),
  });
}

function addEnvironment() {
  emitEnvironment([...environmentEntries(), ["", ""]]);
}

function updateEnvironmentName(index: number, name: string) {
  const entries = environmentEntries();
  entries[index] = [name, entries[index]?.[1] ?? ""];
  emitEnvironment(entries);
}

function updateEnvironmentValue(index: number, value: string) {
  const entries = environmentEntries();
  entries[index] = [entries[index]?.[0] ?? "", value];
  emitEnvironment(entries);
}

function removeEnvironment(index: number) {
  emitEnvironment(environmentEntries().filter((_, position) => position !== index));
}

function portableEnvironmentName(name: string) {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name);
}
</script>

<style scoped>
.command-editor {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.75rem;
}
</style>
