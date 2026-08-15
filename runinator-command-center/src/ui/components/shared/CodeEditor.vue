<template>
  <section class="editor-shell editor-shell--tall" @mousedown.stop @click.stop>
    <header v-if="title" class="editor-shell-title">
      <span>{{ title }}</span>
      <span class="text-xs text-fg-muted">{{ language }}</span>
    </header>
    <div ref="editorContainer" class="editor-shell-container"></div>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getTextEditorHostFactory } from "../../../core/platform";
import { canonicalForeignLanguage } from "../../adapters/codemirror/foreign-language";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    language: string;
    readonly?: boolean;
    title?: string;
  }>(),
  { readonly: false, title: "Source" },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const editorContainer = ref<HTMLElement | null>(null);
let host: ReturnType<ReturnType<typeof getTextEditorHostFactory>["create"]> | null = null;

onMounted(() => {
  if (!editorContainer.value) {
    return;
  }

  const language = canonicalForeignLanguage(props.language) ?? "javascript";
  host = getTextEditorHostFactory().create({
    language,
    value: props.modelValue,
    readonly: props.readonly,
    onChange(value) {
      emit("update:modelValue", value);
    },
  });
  host.mount(editorContainer.value);
});

watch(
  () => props.modelValue,
  (value) => host?.setValue(value, true),
);

watch(
  () => props.readonly,
  (readonly) => host?.setReadonly(readonly),
);

watch(
  () => props.language,
  (language) => {
    const canonical = canonicalForeignLanguage(language);

    if (canonical) {
      host?.setLanguage?.(canonical);
    }
  },
);

onBeforeUnmount(() => {
  host?.destroy();
  host = null;
});
</script>
