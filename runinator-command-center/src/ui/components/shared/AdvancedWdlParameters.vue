<template>
  <details class="advanced-params">
    <summary>{{ title }}</summary>
    <ExpressionJsonEditor
      :model-value="modelValue"
      :context="context"
      :title="title"
      @update:model-value="$emit('update:modelValue', $event)"
    />
  </details>
</template>

<script setup lang="ts">
import ExpressionJsonEditor from "./ExpressionJsonEditor.vue";
import type { WorkflowExpressionEditorContext } from "../../../utils/workflow-expression-completion";

// a single collapsible wrapper for the raw-wdl value editors in the step dialog, so every node kind
// shares one "advanced" disclosure instead of repeating the markup inline. it shares the same
// parameters_json string as the structured editors above it; edits are last-write-wins.
withDefaults(
  defineProps<{
    modelValue: string;
    context?: WorkflowExpressionEditorContext;
    title?: string;
  }>(),
  { title: "Advanced WDL parameters", context: undefined },
);

defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<style scoped>
.advanced-params {
  margin-top: 8px;
}

.advanced-params summary {
  cursor: pointer;
  color: #66717e;
  font-size: 12px;
}
</style>
