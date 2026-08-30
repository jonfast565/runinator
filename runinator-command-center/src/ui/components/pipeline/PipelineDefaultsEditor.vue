<template>
  <form class="flex flex-col gap-3" @submit.prevent="save">
    <label class="flex flex-col gap-1 text-[13px]">
      <span>On step failure</span>
      <select v-model="onStepFailure">
        <option value="halt">Halt the pipeline (new links fire on success)</option>
        <option value="continue">Continue the pipeline (new links fire on complete)</option>
      </select>
    </label>

    <div class="grid grid-cols-2 gap-2 border-t border-border-subtle pt-3">
      <label class="flex flex-col gap-1 text-[13px]">
        <span>Max concurrent pipeline runs</span>
        <input v-model.number="maxConcurrentRuns" type="number" min="0" step="1" required />
        <small class="text-fg-muted">0 means unlimited.</small>
      </label>
      <label class="flex flex-col gap-1 text-[13px]">
        <span>At capacity</span>
        <select v-model="onConflict">
          <option value="allow">Allow</option>
          <option value="skip">Skip</option>
          <option value="queue">Queue</option>
          <option value="cancel_previous">Cancel previous</option>
        </select>
      </label>
    </div>

    <label class="flex flex-row items-center gap-2 text-[13px]">
      <input v-model="linksEnabled" type="checkbox" />
      <span>New links enabled by default</span>
    </label>

    <label class="flex flex-col gap-1 text-[13px]">
      <span>Default member failure mode</span>
      <select v-model="defaultFailureMode">
        <option value="stop">Stop &mdash; halt the pipeline, fire no onward links</option>
        <option value="continue">Continue &mdash; fire onward links, fail the pipeline run</option>
        <option value="silently_continue">
          Silently Continue &mdash; fire onward links, don't fail the pipeline run
        </option>
        <option value="inquire">Inquire &mdash; pause and wait for a decision</option>
      </select>
      <p class="text-xs opacity-70 m-0">
        What happens to the pipeline run when a member workflow fails, unless a member overrides
        this on the canvas. Mirrors PowerShell's $ErrorActionPreference.
      </p>
    </label>

    <label class="flex flex-col gap-1 text-[13px]">
      <span>Max chain depth</span>
      <input v-model="maxChainDepth" type="number" min="1" step="1" placeholder="default (32)" />
    </label>

    <div class="flex flex-col gap-1 text-[13px]">
      <span>Default parameters</span>
      <JsonEditor v-model="parametersText" title="Parameters" />
      <p v-if="parametersError" class="error m-0 text-xs">{{ parametersError }}</p>
    </div>

    <div class="flex justify-end gap-2">
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
import type {
  PipelineDefaults,
  PipelineConcurrency,
  PipelineFailurePolicy,
  PipelineMemberFailureMode,
} from "../../../core/domain/models";
import JsonEditor from "../shared/JsonEditor.vue";

const props = defineProps<{ defaults: PipelineDefaults; concurrency: PipelineConcurrency }>();
const emit = defineEmits<{
  save: [defaults: PipelineDefaults, concurrency: PipelineConcurrency];
  cancel: [];
}>();

const onStepFailure = ref<PipelineFailurePolicy>(props.defaults.on_step_failure);
const linksEnabled = ref<boolean>(props.defaults.links_enabled_by_default);
const defaultFailureMode = ref<PipelineMemberFailureMode>(props.defaults.default_failure_mode);
const maxChainDepth = ref<string>(
  props.defaults.max_chain_depth != null ? String(props.defaults.max_chain_depth) : "",
);
const parametersText = ref<string>(JSON.stringify(props.defaults.default_parameters, null, 2));
const parametersError = ref<string | null>(null);
const maxConcurrentRuns = ref(props.concurrency.max_concurrent_runs);
const onConflict = ref<PipelineConcurrency["on_conflict"]>(props.concurrency.on_conflict);

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

  emit(
    "save",
    {
      on_step_failure: onStepFailure.value,
      links_enabled_by_default: linksEnabled.value,
      default_parameters: parameters,
      max_chain_depth: Number.isFinite(parsedDepth) && parsedDepth > 0 ? parsedDepth : null,
      default_failure_mode: defaultFailureMode.value,
    },
    {
      max_concurrent_runs: Math.max(0, Math.trunc(maxConcurrentRuns.value || 0)),
      on_conflict: onConflict.value,
    },
  );
}
</script>
