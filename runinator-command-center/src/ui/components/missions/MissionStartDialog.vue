<template>
  <Modal
    title="New mission"
    description="Recipe inputs are recorded before work begins, so every phase can be audited and resumed safely."
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
          <span class="flex items-center gap-1"
            >Mission recipe
            <HelpBubble label="About mission recipes">
              {{ recipeDescription }}
            </HelpBubble></span
          >
          <select v-model="pipelineId" class="input" required autofocus>
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
        <div v-if="selectedPipeline" class="mission-recipe-summary">
          <div>
            <strong>{{ selectedPipeline.name }}</strong>
            <p>{{ recipeDescription }}</p>
          </div>
          <span v-if="recipe"
            >{{ recipe.phases.length }} phase{{ recipe.phases.length === 1 ? "" : "s" }} · up to
            {{ recipe.maxEpochs }} epochs</span
          >
          <span v-else>Reusable mission recipe</span>
        </div>
      </section>

      <section v-if="recipe" class="mission-start-section">
        <div>
          <p class="mission-start-kicker">2 · Provide inputs</p>
          <h3>Complete this recipe's launch contract</h3>
        </div>
        <label
          v-for="input in recipe.inputs"
          :key="input.path"
          class="field-label"
          :class="{ 'mission-boolean-field': input.kind === 'boolean' }"
        >
          <span>{{ input.label }}</span>
          <input
            v-if="input.kind !== 'boolean' && input.kind !== 'any'"
            v-model="fieldValues[input.path]"
            class="input"
            :type="input.kind === 'integer' || input.kind === 'number' ? 'number' : 'text'"
            :step="input.kind === 'integer' ? 1 : input.kind === 'number' ? 'any' : undefined"
            :required="input.required"
            :placeholder="input.description"
            :autocomplete="input.path.endsWith('.repository') ? 'url' : 'off'"
          />
          <textarea
            v-else-if="input.kind === 'any'"
            class="input min-h-24 font-mono text-xs"
            :value="fieldTextValue(input.path)"
            :required="input.required"
            :placeholder="input.description || 'Enter a JSON value'"
            spellcheck="false"
            @input="updateFieldText(input.path, $event)"
          ></textarea>
          <input v-else v-model="fieldValues[input.path]" type="checkbox" />
          <small v-if="input.description" class="text-fg-muted">{{ input.description }}</small>
        </label>
      </section>

      <template v-else>
        <section class="mission-start-section">
          <div>
            <p class="mission-start-kicker">2 · Define the work</p>
            <h3>Give the operator a clear brief</h3>
          </div>
          <label class="field-label"
            ><span>Objective</span
            ><textarea
              v-model.trim="goal"
              class="input min-h-24"
              placeholder="Describe the outcome, constraints, and checks that matter."
              required
            ></textarea>
          </label>
        </section>
        <section class="mission-start-section">
          <div>
            <p class="mission-start-kicker">3 · Pin the source</p>
            <h3>Start from a known revision</h3>
          </div>
          <div class="mission-source-grid">
            <label class="field-label mission-source-repository">
              <span class="flex items-center gap-1"
                >Repository URL
                <HelpBubble label="About mission source">
                  The worker resolves the supplied revision once and records the resulting commit
                  before work begins.
                </HelpBubble></span
              >
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
        </section>
      </template>

      <details class="mission-advanced">
        <summary>Advanced payload</summary>
        <label class="field-label mt-2">
          <span class="flex items-center gap-1"
            >Mission reference
            <HelpBubble label="About mission references">
              Reusing a stable reference is treated as the same logical mission. A unique reference
              is generated automatically.
            </HelpBubble></span
          >
          <input
            v-model.trim="correlationKey"
            class="input"
            placeholder="Generated automatically"
            autocomplete="off"
          />
        </label>
        <textarea
          v-model="additionalParameters"
          class="input min-h-28 font-mono text-xs"
          placeholder='{"request": {"labels": ["frontend"]}}'
          spellcheck="false"
        ></textarea>
      </details>

      <p v-if="visibleError" class="error m-0 text-sm" role="alert">{{ visibleError }}</p>
    </form>

    <template #actions>
      <Button variant="ghost" :disabled="starting" @click="emit('close')">Cancel</Button>
      <Button
        variant="primary"
        form="start-mission"
        type="submit"
        icon="runs"
        :loading="starting"
        :disabled="!canStart"
      >
        Start mission
      </Button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { asJsonRecord, type JsonRecord, type JsonValue } from "../../../core/domain/json";
import type { Pipeline } from "../../../core/domain/models";
import type { MissionRecipeDraft, StartMissionInput } from "../../../core/services";
import { missionRecipeFromPipeline } from "../../../core/services";
import Button from "../shared/Button.vue";
import HelpBubble from "../shared/HelpBubble.vue";
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
const correlationKey = ref(`mission-${crypto.randomUUID()}`);
const repository = ref("");
const revision = ref("");
const additionalParameters = ref("");
const advancedPayloadError = ref<string | null>(null);
const fieldValues = ref<Partial<Record<string, string | number | boolean>>>({});

const selectedPipeline = computed(() =>
  props.pipelines.find((pipeline) => pipeline.id === pipelineId.value),
);
const recipe = computed<MissionRecipeDraft | null>(() =>
  selectedPipeline.value ? missionRecipeFromPipeline(selectedPipeline.value) : null,
);
const recipeDescription = computed(
  () => selectedPipeline.value?.description ?? "Run the selected reusable mission recipe.",
);
const visibleError = computed(() => advancedPayloadError.value ?? props.error);
const canStart = computed(() => {
  if (!selectedPipeline.value?.id) {
    return false;
  }

  if (!recipe.value) {
    return Boolean(goal.value && repository.value && revision.value);
  }

  return recipe.value.inputs.every((input) => {
    if (!input.required) {
      return true;
    }

    const value = fieldValues.value[input.path];
    return typeof value !== "string" || value.trim().length > 0;
  });
});

watch(
  () => props.pipelines,
  (pipelines) => {
    if (!pipelineId.value || !pipelines.some((pipeline) => pipeline.id === pipelineId.value)) {
      pipelineId.value = pipelines[0]?.id ?? "";
    }

    initializeFields();
  },
  { immediate: true },
);
watch(pipelineId, initializeFields);

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
  let parameters: JsonRecord;

  try {
    parameters = recipe.value
      ? applyFieldValues(additional)
      : {
          ...additional,
          request: { ...extraRequest, goal: goal.value },
          mission: {
            ...extraMission,
            source: { ...extraSource, repository: repository.value, revision: revision.value },
          },
        };
  } catch (cause) {
    advancedPayloadError.value = cause instanceof Error ? cause.message : String(cause);
    return;
  }

  emit("start", {
    pipelineId: pipeline.id,
    correlationKey: correlationKey.value || `mission-${crypto.randomUUID()}`,
    parameters,
  });
}

function initializeFields(): void {
  fieldValues.value = Object.fromEntries(
    (recipe.value?.inputs ?? []).map((input) => {
      const fallback = input.kind === "boolean" ? false : "";
      const value = ["string", "number", "boolean"].includes(typeof input.defaultValue)
        ? (input.defaultValue as string | number | boolean)
        : fallback;
      return [input.path, value];
    }),
  );
}

function fieldTextValue(path: string): string {
  const value = fieldValues.value[path];
  return typeof value === "string" || typeof value === "number" ? String(value) : "";
}

function updateFieldText(path: string, event: Event): void {
  fieldValues.value[path] = (event.target as HTMLTextAreaElement).value;
}

function applyFieldValues(base: JsonRecord): JsonRecord {
  const result = structuredClone(base);

  for (const input of recipe.value?.inputs ?? []) {
    const raw = fieldValues.value[input.path];

    if (input.required && typeof raw === "string" && !raw.trim()) {
      throw new Error(`${input.label} is required.`);
    }

    let value: JsonValue = raw ?? null;

    if ((input.kind === "integer" || input.kind === "number") && raw !== "") {
      value = Number(raw);

      if (!Number.isFinite(value) || (input.kind === "integer" && !Number.isInteger(value))) {
        throw new Error(`${input.label} must be a valid ${input.kind}.`);
      }
    } else if (input.kind === "any" && typeof raw === "string" && raw.trim()) {
      try {
        value = JSON.parse(raw) as JsonValue;
      } catch {
        throw new Error(`${input.label} must be valid JSON.`);
      }
    }

    setPath(result, input.path, value);
  }

  return result;
}

function setPath(target: JsonRecord, path: string, value: JsonValue): void {
  const parts = path.split(".").filter(Boolean);
  let current = target;

  for (const part of parts.slice(0, -1)) {
    current[part] = asJsonRecord(current[part]);
    current = current[part] as JsonRecord;
  }

  const leaf = parts.at(-1);

  if (leaf) {
    current[leaf] = value;
  }
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
.mission-recipe-summary {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.8rem;
  border-top: 1px solid var(--color-border-subtle);
  padding-top: 0.65rem;
}
.mission-recipe-summary div {
  min-width: 0;
}
.mission-recipe-summary strong {
  color: var(--color-fg);
  font-size: 0.78rem;
}
.mission-recipe-summary p {
  margin: 0.15rem 0 0;
  color: var(--color-fg-muted);
  font-size: 0.72rem;
  line-height: 1.4;
}
.mission-recipe-summary > span {
  flex: none;
  border-radius: 999px;
  padding: 0.2rem 0.5rem;
  color: var(--color-fg-muted);
  background: var(--color-bg-subtle);
  font-size: 0.68rem;
  white-space: nowrap;
}
.mission-boolean-field {
  grid-template-columns: auto 1fr;
  align-items: center;
}
.mission-boolean-field > span {
  grid-column: 2;
  grid-row: 1;
}
.mission-boolean-field > input {
  grid-column: 1;
  grid-row: 1;
}
.mission-boolean-field > small {
  grid-column: 2;
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
  .mission-recipe-summary {
    flex-direction: column;
  }
}
</style>
