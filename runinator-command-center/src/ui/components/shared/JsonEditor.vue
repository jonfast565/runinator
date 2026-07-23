<template>
  <section class="editor-shell editor-shell--tall json-editor-shell" @mousedown.stop @click.stop>
    <header v-if="title" class="editor-shell-title">
      <span>{{ title }}</span>
      <button
        type="button"
        class="editor-shell-title-btn"
        :title="copied ? 'Copied' : 'Copy JSON'"
        @click="copy"
      >
        {{ copied ? "Copied" : "Copy" }}
      </button>
    </header>
    <div ref="editorContainer" class="editor-shell-container"></div>
    <p v-if="!readonly && parseError" class="editor-shell-error" role="alert">
      <Icon name="alert" :size="12" />
      <span>{{ parseError }}</span>
    </p>
  </section>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from "vue";
import { getTextEditorHostFactory } from "../../../core/platform";
import type { CodeMirrorHostOptions } from "../../adapters/codemirror/text-editor-host";
import Icon from "./Icon.vue";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    readonly?: boolean;
    keyHints?: string[];
    title?: string;
  }>(),
  { title: "JSON", keyHints: undefined },
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const editorContainer = ref<HTMLElement | null>(null);
const title = props.title;
const copied = ref(false);
const parseError = ref("");
let host: ReturnType<ReturnType<typeof getTextEditorHostFactory>["create"]> | null = null;

function validate(text: string) {
  const trimmed = text.trim();

  if (!trimmed) {
    parseError.value = "";
    return;
  }

  try {
    JSON.parse(trimmed);
    parseError.value = "";
  } catch (err) {
    parseError.value = err instanceof Error ? err.message : "Invalid JSON";
  }
}

async function copy() {
  try {
    await navigator.clipboard.writeText(props.modelValue);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1200);
  } catch {
    // clipboard may be unavailable; ignore.
  }
}

onMounted(() => {
  if (!editorContainer.value) {
    return;
  }

  const options: CodeMirrorHostOptions = {
    language: "json",
    value: props.modelValue,
    readonly: props.readonly,
    jsonKeyHints: () => props.keyHints ?? [],
    onChange(text) {
      emit("update:modelValue", text);

      if (!props.readonly) {
        validate(text);
      }
    },
  };

  host = getTextEditorHostFactory().create(options);
  host.mount(editorContainer.value);

  if (!props.readonly) {
    validate(props.modelValue);
  }
});

watch(
  () => props.modelValue,
  (newValue) => {
    host?.setValue(newValue, true);

    if (!props.readonly) {
      validate(newValue);
    }
  },
);

watch(
  () => props.readonly,
  (readonly) => {
    host?.setReadonly(Boolean(readonly));
  },
);

onBeforeUnmount(() => {
  host?.destroy();
  host = null;
});
</script>
