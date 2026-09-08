<template>
  <section class="flex flex-wrap items-center gap-2 rounded border border-border-subtle p-3">
    <strong>Orchestration transitions</strong>
    <span class="text-sm text-fg-muted">{{
      paused ? "Paused before the next event or due intent" : "Running"
    }}</span>
    <button class="btn btn-sm" :disabled="busy" @click="change(!paused)">
      {{ paused ? "Resume" : "Pause" }}
    </button>
    <button class="btn btn-sm" :disabled="busy || !paused" @click="change(true, 1)">
      Step once
    </button>
    <span v-if="error" role="alert" class="text-sm text-danger">{{ error }}</span>
  </section>
</template>
<script setup lang="ts">
import { ref, watch } from "vue";
import {
  fetchOrchestrationDebugControl,
  setOrchestrationDebugControl,
} from "../../../core/api/commandCenterApi";
const props = defineProps<{ pipelineId: string }>();
const paused = ref(false);
const busy = ref(false);
const error = ref("");
watch(
  () => props.pipelineId,
  async (id) => {
    error.value = "";

    try {
      const state = await fetchOrchestrationDebugControl(id);

      if (id === props.pipelineId) {
        paused.value = state.paused;
      }
    } catch (cause) {
      error.value = String(cause);
    }
  },
  { immediate: true },
);

async function change(value: boolean, steps = 0) {
  busy.value = true;
  error.value = "";

  try {
    await setOrchestrationDebugControl(props.pipelineId, value, steps);
    paused.value = value;
  } catch (cause) {
    error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
</script>
