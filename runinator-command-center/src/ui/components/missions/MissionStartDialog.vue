<template>
  <Modal
    title="Start a mission"
    description="The source and objective are recorded before work begins, so every phase can be audited and resumed safely."
    width="min(720px, 100%)"
    @close="emit('close')"
  >
    <form id="start-mission" class="mission-start-form" @submit.prevent="submit">
      <section class="mission-start-section">
        <div>
          <p class="mission-start-kicker">1 · Choose a recipe</p>
          <h3>What should this mission do?</h3>
        </div>
        <label class="field-label">
          <span>Mission recipe</span>
          <select v-model="pipelineId" class="input" required>
            <option value="" disabled>Choose a mission recipe…</option>
            <option
              v-for="pipeline in pipelines"
              :key="pipeline.id ?? ''"
              :value="pipeline.id ?? ''"
            >
              {{ pipeline.name }}
            </option>
          </select>
        </label>
        <p v-if="selectedPipeline" class="mission-start-help">
          {{ selectedPipeline.description || recipeDescription }}
        </p>
      </section>

      <section class="mission-start-section">
        <div>
          <p class="mission-start-kicker">2 · Define the work</p>
          <h3>Give the operator a clear brief</h3>
        </div>
        <label class="field-label">
          <span>Objective</span>
          <textarea
            v-model.trim="goal"
            class="input min-h-24"
            placeholder="Describe the outcome, constraints, and checks that matter."
            required
          ></textarea>
        </label>
        <label class="field-label">
          <span>Mission reference</span>
          <input
            v-model.trim="correlationKey"
            class="input"
            placeholder="feature-auth-refresh"
            autocomplete="off"
            required
          />
        </label>
        <p class="mission-start-help">
          Use a stable, unique reference. Reusing one is treated as the same logical mission.
        </p>
      </section>

      <section class="mission-start-section">
        <div>
          <p class="mission-start-kicker">3 · Pin the source</p>
          <h3>Start from a known revision</h3>
        </div>
        <div class="mission-source-grid">
          <label class="field-label mission-source-repository">
            <span>Repository URL</span>
            <input
              v-model.trim="repository"
              class="input"
              placeholder="https://github.com/org/repository.git"
              autocomplete="url"
              required
            />
          </label>
          <label class="field-label">
            <span>Revision</span>
            <input
              v-model.trim="revision"
              class="input"
              placeholder="Commit SHA, branch, or tag"
              autocomplete="off"
              required
            />
          </label>
        </div>
        <p class="mission-start-help">
          The worker resolves this once and records the resulting commit before it begins.
        </p>
      </section>

      <details class="mission-advanced">
        <summary>Advanced payload</summary>
        <p>
          Add optional JSON fields for a recipe-specific input. The objective and source above stay
          authoritative.
        </p>
        <textarea
          v-model="additionalParameters"
          class="input min-h-28 font-mono text-xs"
          placeholder='{"request": {"labels": ["frontend"]}}'
          spellcheck="false"
        ></textarea>
      </details>

      <p v-if="visibleError" class="error m-0 text-sm">{{ visibleError }}</p>
    </form>

    <template #actions>
      <Button variant="ghost" @click="emit('close')">Cancel</Button>
      <Button variant="primary" form="start-mission" type="submit" icon="runs" :loading="starting">
        Start {{ recipeLabel }} mission
      </Button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { asJsonRecord, type JsonRecord } from "../../../core/domain/json";
import type { Pipeline } from "../../../core/domain/models";
import type { MissionKind, StartMissionInput } from "../../../core/services";
import Button from "../shared/Button.vue";
import Modal from "../shared/Modal.vue";

const props = withDefaults(
  defineProps<{
    pipelines: Pipeline[];
    starting?: boolean;
    error?: string | null;
  }>(),
  {
    starting: false,
    error: null,
  },
);

const emit = defineEmits<{
  close: [];
  start: [input: StartMissionInput];
}>();

const pipelineId = ref("");
const goal = ref("");
const correlationKey = ref("");
const repository = ref("");
const revision = ref("");
const additionalParameters = ref("");
const advancedPayloadError = ref<string | null>(null);

const selectedPipeline = computed(() =>
  props.pipelines.find((pipeline) => pipeline.id === pipelineId.value),
);
const kind = computed<MissionKind>(() => {
  const scope = asJsonRecord(selectedPipeline.value?.metadata.ingress).scope;
  return scope === "mission.research_report" ? "research_report" : "coding";
});
const recipeLabel = computed(() => (kind.value === "coding" ? "coding" : "research/report"));
const recipeDescription = computed(() =>
  kind.value === "coding"
    ? "Implement, independently review, verify, and hand off a bounded code change."
    : "Investigate a question, independently critique the evidence, and produce a durable report.",
);
const visibleError = computed(() => advancedPayloadError.value ?? props.error);

watch(
  () => props.pipelines,
  (pipelines) => {
    if (!pipelineId.value || !pipelines.some((pipeline) => pipeline.id === pipelineId.value)) {
      pipelineId.value = pipelines[0]?.id ?? "";
    }
  },
  { immediate: true },
);

function submit(): void {
  const pipeline = selectedPipeline.value;

  if (!pipeline?.id) {
    return;
  }

  advancedPayloadError.value = null;
  let additional: JsonRecord;

  try {
    additional = parseAdditionalParameters();
  } catch (cause) {
    advancedPayloadError.value =
      cause instanceof Error ? cause.message : "Advanced payload is not valid JSON.";
    return;
  }

  const extraRequest = asJsonRecord(additional.request);
  const extraMission = asJsonRecord(additional.mission);
  const extraSource = asJsonRecord(extraMission.source);
  emit("start", {
    pipelineId: pipeline.id,
    kind: kind.value,
    correlationKey: correlationKey.value,
    parameters: {
      ...additional,
      request: { ...extraRequest, goal: goal.value },
      mission: {
        ...extraMission,
        source: {
          ...extraSource,
          repository: repository.value,
          revision: revision.value,
        },
      },
    },
  });
}

function parseAdditionalParameters(): JsonRecord {
  if (!additionalParameters.value.trim()) {
    return {};
  }

  const parsed: unknown = JSON.parse(additionalParameters.value);

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("Advanced payload must be a JSON object.");
  }

  return parsed as JsonRecord;
}
</script>

<style scoped>
.mission-start-form {
  display: grid;
  gap: 1rem;
}
.mission-start-section {
  display: grid;
  gap: 0.6rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.7rem;
  padding: 0.85rem;
  background: var(--color-bg-subtle);
}
.mission-start-kicker {
  margin: 0;
  color: var(--color-accent-text);
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.mission-start-section h3 {
  margin: 0.12rem 0 0;
  color: var(--color-fg);
  font-size: 0.9rem;
}
.mission-start-help,
.mission-advanced p {
  margin: 0;
  color: var(--color-fg-muted);
  font-size: 0.75rem;
  line-height: 1.45;
}
.mission-source-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(10rem, 0.8fr);
  gap: 0.65rem;
}
.mission-advanced {
  display: grid;
  gap: 0.6rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.7rem;
  padding: 0.7rem 0.85rem;
}
.mission-advanced summary {
  cursor: pointer;
  color: var(--color-fg);
  font-size: 0.8rem;
  font-weight: 600;
}
.mission-advanced[open] summary {
  margin-bottom: 0.1rem;
}
@media (max-width: 600px) {
  .mission-source-grid {
    grid-template-columns: 1fr;
  }
}
</style>
