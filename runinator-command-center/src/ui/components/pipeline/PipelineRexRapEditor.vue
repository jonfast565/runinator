<template>
  <section class="pipeline-rexrap-editor grid gap-2">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div>
        <h3 class="m-0 text-sm font-semibold text-fg">Pipeline REXRAP</h3>
        <p class="m-0 mt-0.5 text-xs text-fg-muted">
          Source and the visual pipeline stay in sync. Applying source creates the next pipeline
          revision.
        </p>
      </div>
      <div class="btn-row">
        <button class="btn btn-sm" :disabled="loading || applying" @click="refresh">
          <Icon name="refresh" :size="14" />
          <span>Reload</span>
        </button>
        <button class="btn btn-primary btn-sm" :disabled="loading || applying" @click="apply">
          <LoadingSpinner v-if="applying" size="sm" label="Applying pipeline source" />
          <Icon v-else name="check" :size="14" />
          <span>{{ applying ? "Applying…" : "Apply REXRAP" }}</span>
        </button>
      </div>
    </div>

    <p v-if="error" class="m-0 text-xs text-danger-fg" role="alert">{{ error }}</p>
    <RexRapEditor
      v-if="!loading"
      v-model="source"
      class="pipeline-rexrap-source"
      title="Pipeline REXRAP"
      document="pipeline"
    />
    <div v-else class="grid min-h-52 place-items-center rounded border border-border bg-surface">
      <LoadingSpinner label="Loading pipeline REXRAP" />
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import type { Pipeline } from "../../../core/domain/models";
import { fetchPipelineRexRap } from "../../../core/services/pipeline";
import { useAppStore } from "../../adapters/pinia/app";
import { usePipelineStore } from "../../adapters/pinia/pipeline";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import RexRapEditor from "../shared/RexRapEditor.vue";

const props = defineProps<{ pipeline: Pipeline }>();

const app = useAppStore();
const pipelines = usePipelineStore();
const source = ref("");
const loading = ref(false);
const applying = ref(false);
const error = ref<string | null>(null);
let refreshRequest = 0;

async function refresh(): Promise<void> {
  const id = props.pipeline.id;

  if (!id) {
    error.value = "Save the pipeline before editing its REXRAP source.";
    return;
  }

  const request = ++refreshRequest;
  loading.value = true;
  error.value = null;

  try {
    const next = await fetchPipelineRexRap(id);

    if (request === refreshRequest) {
      source.value = next;
    }
  } catch (cause) {
    if (request === refreshRequest) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    }
  } finally {
    if (request === refreshRequest) {
      loading.value = false;
    }
  }
}

async function apply(): Promise<void> {
  const id = props.pipeline.id;

  if (!id) {
    error.value = "Save the pipeline before applying REXRAP.";
    return;
  }

  applying.value = true;
  error.value = null;
  const saved = await pipelines.savePipelineRexRapFor(id, source.value);
  applying.value = false;

  if (!saved) {
    error.value = pipelines.error ?? "Could not apply pipeline REXRAP.";
    return;
  }

  app.setStatus(`Applied REXRAP for ${props.pipeline.name}`);
  await refresh();
}

// Every visual or metadata save replaces the pipeline object in the store. Regenerate source from
// that durable revision so neither editor becomes a stale competing representation.
watch(
  () => props.pipeline,
  () => {
    if (!applying.value) {
      void refresh();
    }
  },
  { immediate: true },
);
</script>

<style scoped>
.pipeline-rexrap-source {
  min-height: 28rem;
}

.pipeline-rexrap-source :deep(.rexrap-editor-container) {
  min-height: 18rem;
}
</style>
