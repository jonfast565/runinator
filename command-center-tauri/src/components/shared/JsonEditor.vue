<template>
  <textarea :value="modelValue" spellcheck="false" @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)" />
</template>

<script setup lang="ts">
defineProps<{ modelValue: string }>();
defineEmits<{ "update:modelValue": [value: string] }>();
</script>
