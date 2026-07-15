<template>
  <form class="pipeline-defaults" @submit.prevent="save">
    <label class="pipeline-field">
      <span>On step failure</span>
      <select v-model="onStepFailure">
        <option value="halt">Halt the pipeline (new links fire on success)</option>
        <option value="continue">Continue the pipeline (new links fire on complete)</option>
      </select>
    </label>

    <label class="pipeline-field pipeline-field-inline">
      <input v-model="linksEnabled" type="checkbox" />
      <span>New links enabled by default</span>
    </label>

    <label class="pipeline-field">
      <span>Max chain depth</span>
      <input
        v-model="maxChainDepth"
        type="number"
        min="1"
        placeholder="default (32)"
      />
    </label>

    <div class="pipeline-field">
      <span>Default parameters</span>
      <JsonEditor v-model="parametersText" title="Parameters" />
      <p v-if="parametersError" class="pipeline-defaults-error">{{ parametersError }}</p>
    </div>

    <div class="pipeline-defaults-actions">
      <button type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button type="submit" class="btn btn-primary" :disabled="Boolean(parametersError)">
        Save defaults
      </button>
    </div>
  </form>
</template>

<script setup lang="ts">
import { ref } from "vue";
import type { JsonRecord } from "../../../core/domain/json";
import type { PipelineDefaults, PipelineFailurePolicy } from "../../../core/domain/models";
import JsonEditor from "../shared/JsonEditor.vue";

const props = defineProps<{ defaults: PipelineDefaults }>();
const emit = defineEmits<{ save: [defaults: PipelineDefaults]; cancel: [] }>();

const onStepFailure = ref<PipelineFailurePolicy>(props.defaults.on_step_failure);
const linksEnabled = ref<boolean>(props.defaults.links_enabled_by_default);
const maxChainDepth = ref<string>(
  props.defaults.max_chain_depth != null ? String(props.defaults.max_chain_depth) : "",
);
const parametersText = ref<string>(
  JSON.stringify(props.defaults.default_parameters, null, 2),
);
const parametersError = ref<string | null>(null);

function parseParameters(): JsonRecord | null {
  const raw = parametersText.value.trim();

  if (!raw) {
    return {};
  }

  try {
    const parsed = JSON.parse(raw) as unknown;

    if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
      parametersError.value = "Default parameters must be a JSON object.";
      return null;
    }

    parametersError.value = null;
    return parsed as JsonRecord;
  } catch (err) {
    parametersError.value = err instanceof Error ? err.message : "Invalid JSON.";
    return null;
  }
}

function save() {
  const parameters = parseParameters();

  if (parameters == null) {
    return;
  }

  const depth = maxChainDepth.value.trim();
  const parsedDepth = depth ? Number.parseInt(depth, 10) : Number.NaN;

  emit("save", {
    on_step_failure: onStepFailure.value,
    links_enabled_by_default: linksEnabled.value,
    default_parameters: parameters,
    max_chain_depth: Number.isFinite(parsedDepth) && parsedDepth > 0 ? parsedDepth : null,
  });
}
</script>

<style scoped>
.pipeline-defaults {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.pipeline-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}

.pipeline-field-inline {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.pipeline-defaults-error {
  color: var(--danger, #b42318);
  font-size: 12px;
  margin: 0;
}

.pipeline-defaults-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
