<template>
  <textarea
    :value="modelValue"
    spellcheck="false"
    class="json-textarea"
    @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
  />
</template>

<script setup lang="ts">
defineProps<{ modelValue: string }>();
defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<style scoped>
.json-textarea {
  height: 100%;
  resize: none;
  overflow-y: auto;
}
</style>
