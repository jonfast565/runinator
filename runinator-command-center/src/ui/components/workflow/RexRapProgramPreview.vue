<template>
  <div class="overflow-hidden rounded-lg border border-border bg-surface-subtle">
    <div v-if="loading" class="flex items-center gap-2 px-3 py-3 text-xs text-fg-muted">
      <LoadingSpinner size="sm" label="Rendering invocation program" />
      Rendering program…
    </div>
    <p v-else-if="error" class="m-0 px-3 py-3 text-xs text-danger-fg" role="alert">
      {{ error }}
    </p>
    <pre
      v-else
      class="m-0 max-h-80 overflow-auto whitespace-pre p-3 font-mono text-xs leading-5 text-fg"
    ><code>{{ source }}</code></pre>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { rexrapLanguageService } from "../../../core/services";
import LoadingSpinner from "../shared/LoadingSpinner.vue";

const props = defineProps<{ program: unknown }>();

const source = ref("");
const error = ref("");
const loading = ref(false);
let renderGeneration = 0;

watch(
  () => props.program,
  async (program) => {
    const generation = ++renderGeneration;
    loading.value = true;
    error.value = "";

    try {
      const rendered = await rexrapLanguageService.renderProgram(program);

      if (generation === renderGeneration) {
        source.value = rendered;
      }
    } catch (err) {
      if (generation === renderGeneration) {
        error.value = `Could not render this invocation program: ${String(err)}`;
      }
    } finally {
      if (generation === renderGeneration) {
        loading.value = false;
      }
    }
  },
  { immediate: true },
);
</script>
