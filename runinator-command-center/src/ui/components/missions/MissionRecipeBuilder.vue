<template>
  <Modal
    title="Mission recipe builder"
    width="min(980px, 100%)"
    :close-on-backdrop="false"
    @close="emit('close')"
  >
    <div class="recipe-steps" aria-label="Recipe builder progress">
      <button
        v-for="(label, index) in steps"
        :key="label"
        type="button"
        :class="{ 'is-active': step === index }"
        @click="step = index"
      >
        <span>{{ index + 1 }}</span
        >{{ label }}
      </button>
    </div>

    <form id="mission-recipe-form" class="recipe-form" @submit.prevent="submit">
      <section v-if="step === 0" class="recipe-section">
        <h3>Start with a preset</h3>
        <p>Presets are ordinary editable drafts. Selecting one never starts model work.</p>
        <div class="preset-grid">
          <button
            v-for="preset in presets"
            :key="preset.id"
            type="button"
            :class="{ selected: draft.preset === preset.id }"
            @click="selectPreset(preset.id)"
          >
            <strong>{{ preset.label }}</strong
            ><span>{{ preset.description }}</span>
          </button>
        </div>
        <div class="grid gap-3 md:grid-cols-2">
          <label class="field-label"
            ><span>Name</span><input v-model.trim="draft.name" class="input" required
          /></label>
          <label class="field-label"
            ><span>Key</span><input v-model.trim="draft.key" class="input" required
          /></label>
          <label class="field-label"
            ><span>Namespace</span><input v-model.trim="draft.namespace" class="input" required
          /></label>
          <label class="field-label"
            ><span>Maximum epochs</span
            ><input v-model.number="draft.maxEpochs" class="input" type="number" min="1" required
          /></label>
        </div>
        <label class="field-label"
          ><span>Description</span
          ><textarea v-model.trim="draft.description" class="input min-h-20"></textarea>
        </label>
      </section>

      <section v-else-if="step === 1" class="recipe-section">
        <div class="section-heading">
          <div>
            <h3>Launch inputs</h3>
            <p>These fields become the reusable launch form and workflow input contract.</p>
          </div>
          <Button type="button" icon="plus" @click="addInput">Add input</Button>
        </div>
        <article
          v-for="(input, index) in draft.inputs"
          :key="`${input.path}-${index}`"
          class="input-card"
        >
          <div class="grid gap-2 md:grid-cols-[1.2fr_1fr_0.7fr_auto]">
            <label class="field-label"
              ><span>Path</span
              ><input v-model.trim="input.path" class="input" placeholder="request.goal"
            /></label>
            <label class="field-label"
              ><span>Label</span><input v-model.trim="input.label" class="input"
            /></label>
            <label class="field-label"
              ><span>Type</span
              ><select v-model="input.kind" class="input">
                <option v-for="kind in inputKinds" :key="kind" :value="kind">{{ kind }}</option>
              </select></label
            >
            <label class="check-field"
              ><input v-model="input.required" type="checkbox" /> Required</label
            >
          </div>
          <div class="flex items-end gap-2">
            <label class="field-label flex-1"
              ><span>Description</span
              ><input v-model.trim="input.description" class="input" /></label
            ><Button type="button" variant="ghost" @click="draft.inputs.splice(index, 1)"
              >Remove</Button
            >
          </div>
        </article>
      </section>

      <section v-else-if="step === 2" class="recipe-section">
        <div class="section-heading">
          <div>
            <h3>Phase graph</h3>
            <p>
              Each phase is a generated workflow; routes may form bounded review and repair loops.
            </p>
          </div>
          <Button type="button" icon="plus" @click="addPhase">Add phase</Button>
        </div>
        <div v-if="draft.phases.some((phase) => phase.workspace)" class="grid gap-2 md:grid-cols-2">
          <label class="field-label"
            ><span>Shared workspace scope</span
            ><input v-model.trim="draft.workspaceScope" class="input" placeholder="mission-source"
          /></label>
          <label class="field-label"
            ><span>Workspace lease seconds</span
            ><input
              v-model.number="draft.workspaceLeaseSeconds"
              class="input"
              type="number"
              min="1"
            />
          </label>
        </div>
        <article
          v-for="(phase, index) in draft.phases"
          :key="`${phase.id}-${index}`"
          class="phase-card"
        >
          <div class="phase-card-heading">
            <strong>{{ index + 1 }} · {{ phase.name || "Unnamed phase" }}</strong
            ><Button
              type="button"
              variant="ghost"
              :disabled="draft.phases.length === 1"
              @click="draft.phases.splice(index, 1)"
              >Remove</Button
            >
          </div>
          <div class="grid gap-2 md:grid-cols-3">
            <label class="field-label"
              ><span>ID</span><input v-model.trim="phase.id" class="input"
            /></label>
            <label class="field-label"
              ><span>Name</span><input v-model.trim="phase.name" class="input"
            /></label>
            <label class="field-label"
              ><span>Role</span><input v-model.trim="phase.role" class="input"
            /></label>
            <label class="field-label"
              ><span>Provider</span
              ><select v-model="phase.provider" class="input" @change="selectProvider(phase)">
                <option value="">Choose…</option>
                <option
                  v-for="provider in phaseProviders"
                  :key="provider.name"
                  :value="provider.name"
                >
                  {{ provider.name }}
                </option>
              </select></label
            >
            <label class="field-label"
              ><span>Action</span
              ><select v-model="phase.action" class="input" @change="selectAction(phase)">
                <option value="">Choose…</option>
                <option
                  v-for="action in actionsFor(phase.provider)"
                  :key="action.function_name"
                  :value="action.function_name"
                >
                  {{ action.function_name }}
                </option>
              </select></label
            >
            <label class="field-label"
              ><span>Execution profile</span
              ><input v-model.trim="phase.profile" class="input" placeholder="Optional"
            /></label>
          </div>
          <label v-if="phase.promptParameter" class="field-label"
            ><span>Prompt template</span
            ><textarea
              v-model="phase.prompt"
              class="input min-h-24"
              placeholder="Use ${params.request.goal} to bind mission inputs."
            ></textarea>
          </label>
          <div v-if="editableParameters(phase).length" class="grid gap-2 md:grid-cols-2">
            <label
              v-for="parameter in editableParameters(phase)"
              :key="parameter.name"
              class="field-label"
            >
              <span>{{ parameter.label || parameter.name }}</span>
              <input
                v-if="['integer', 'number'].includes(parameter.ty.type)"
                v-model.number="phase.actionParameters[parameter.name]"
                class="input"
                type="number"
                :placeholder="parameter.description || ''"
              />
              <input
                v-else-if="parameter.ty.type !== 'boolean'"
                v-model="phase.actionParameters[parameter.name]"
                class="input"
                type="text"
                :placeholder="
                  parameter.secret ? '=settings.scope.name' : parameter.description || ''
                "
              />
              <input v-else v-model="phase.actionParameters[parameter.name]" type="checkbox" />
              <small v-if="parameter.description" class="text-fg-muted">{{
                parameter.description
              }}</small>
            </label>
          </div>
          <div class="grid gap-2 md:grid-cols-3">
            <label class="field-label"
              ><span>Timeout seconds</span
              ><input v-model.number="phase.timeoutSeconds" class="input" type="number" min="1"
            /></label>
            <label class="field-label"
              ><span>Routing</span
              ><select v-model="phase.routeMode" class="input">
                <option value="fixed">Continue to one phase</option>
                <option value="result">Agent chooses an allowed phase</option>
                <option value="terminal">Terminal</option>
              </select></label
            >
            <label v-if="phase.routeMode === 'fixed'" class="field-label"
              ><span>Next phase</span
              ><select v-model="phase.nextPhase" class="input">
                <option
                  v-for="candidate in otherPhases(phase.id)"
                  :key="candidate.id"
                  :value="candidate.id"
                >
                  {{ candidate.name }}
                </option>
              </select></label
            >
          </div>
          <fieldset v-if="phase.routeMode === 'result'" class="route-targets">
            <legend>Allowed next phases</legend>
            <label v-for="candidate in otherPhases(phase.id)" :key="candidate.id"
              ><input v-model="phase.allowedNextPhases" type="checkbox" :value="candidate.id" />{{
                candidate.name
              }}</label
            >
          </fieldset>
          <div class="flex flex-wrap gap-4 text-sm">
            <label><input v-model="phase.workspace" type="checkbox" /> Shared workspace</label
            ><label
              ><input v-model="phase.retainEvidence" type="checkbox" /> Retain output and
              evidence</label
            >
          </div>
          <details>
            <summary>Advanced action parameters</summary>
            <textarea
              class="input mt-2 min-h-24 font-mono text-xs"
              :value="parameterText(phase)"
              @change="updateParameters(phase, $event)"
            ></textarea>
          </details>
        </article>
      </section>

      <section v-else class="recipe-section">
        <h3>Review</h3>
        <dl class="review-grid">
          <div>
            <dt>Recipe</dt>
            <dd>{{ draft.namespace }}.{{ draft.key }}</dd>
          </div>
          <div>
            <dt>Inputs</dt>
            <dd>{{ draft.inputs.length }}</dd>
          </div>
          <div>
            <dt>Phases</dt>
            <dd>{{ draft.phases.length }}</dd>
          </div>
          <div>
            <dt>Epoch limit</dt>
            <dd>{{ draft.maxEpochs }}</dd>
          </div>
        </dl>
        <ul v-if="issues.length" class="error-list">
          <li v-for="issue in issues" :key="issue">{{ issue }}</li>
        </ul>
        <p v-else class="success-note">The recipe is ready to compile and save atomically.</p>
      </section>
      <p v-if="error" class="error m-0">{{ error }}</p>
    </form>

    <template #actions>
      <Button variant="ghost" @click="emit('close')">Cancel</Button>
      <Button v-if="step > 0" @click="step -= 1">Back</Button>
      <Button v-if="step < steps.length - 1" variant="primary" @click="step += 1">Next</Button>
      <Button
        v-else
        form="mission-recipe-form"
        type="submit"
        variant="primary"
        :loading="saving"
        :disabled="issues.length > 0"
        >Save recipe</Button
      >
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { ActionMetadata, Pipeline, ProviderMetadata } from "../../../core/domain/models";
import type {
  MissionInputKind,
  MissionPhaseDraft,
  MissionPresetId,
  MissionRecipeDraft,
} from "../../../core/services";
import { missionRecipePreset, validateMissionRecipe } from "../../../core/services";
import Button from "../shared/Button.vue";
import Modal from "../shared/Modal.vue";

const props = withDefaults(
  defineProps<{
    providers: ProviderMetadata[];
    pipelines: Pipeline[];
    initial?: MissionRecipeDraft | null;
    saving?: boolean;
    error?: string | null;
  }>(),
  { initial: null, saving: false, error: null },
);
const emit = defineEmits<{ close: []; save: [draft: MissionRecipeDraft] }>();
const steps = ["Preset", "Inputs", "Phases", "Review"];
const presets: { id: MissionPresetId; label: string; description: string }[] = [
  { id: "blank", label: "Blank", description: "Start with one terminal agent phase." },
  { id: "coding", label: "Coding", description: "Implement, review, verify, and summarize." },
  {
    id: "research_report",
    label: "Research/report",
    description: "Investigate, critique, and report.",
  },
];
const inputKinds: MissionInputKind[] = ["string", "integer", "number", "boolean", "any"];
const step = ref(0);
const draft = ref<MissionRecipeDraft>(
  props.initial ? structuredClone(props.initial) : missionRecipePreset("blank"),
);
const initialPath = props.initial ? `${props.initial.namespace}.${props.initial.key}` : null;
const issues = computed(() => {
  const found = validateMissionRecipe(draft.value);
  const path = `${draft.value.namespace}.${draft.value.key}`;

  if (
    path !== initialPath &&
    props.pipelines.some(
      (pipeline) =>
        [pipeline.namespace, pipeline.key ?? pipeline.name].filter(Boolean).join(".") === path,
    )
  ) {
    found.push(`A pipeline already uses '${path}'. Choose another namespace or key.`);
  }

  for (const phase of draft.value.phases) {
    const action = actionsFor(phase.provider).find(
      (candidate) => candidate.function_name === phase.action,
    );

    if (!action) {
      found.push(`Phase '${phase.id}' selects an unavailable provider action.`);
      continue;
    }

    if (phase.routeMode === "result" && !action.agent) {
      found.push(`Phase '${phase.id}' needs an agent-capable action for result routing.`);
    }

    for (const parameter of action.parameters) {
      if (parameter.name === phase.promptParameter) {
        continue;
      }

      const value = phase.actionParameters[parameter.name];

      if (parameter.required && (value === undefined || value === null || value === "")) {
        found.push(`Phase '${phase.id}' requires action parameter '${parameter.name}'.`);
      }

      if (
        parameter.secret &&
        value !== undefined &&
        value !== null &&
        value !== "" &&
        (typeof value !== "string" || !value.startsWith("="))
      ) {
        found.push(
          `Phase '${phase.id}' secret '${parameter.name}' must use an expression reference.`,
        );
      }
    }
  }

  return found;
});
const phaseProviders = computed(() =>
  props.providers.filter((provider) => provider.actions.some((action) => !action.pure)),
);

function selectPreset(id: MissionPresetId): void {
  draft.value = missionRecipePreset(id);
}

function addInput(): void {
  draft.value.inputs.push({
    path: "request.input",
    label: "Input",
    description: "",
    kind: "string",
    required: true,
  });
}

function addPhase(): void {
  const id = `phase_${String(draft.value.phases.length + 1)}`;
  draft.value.phases.push({
    id,
    name: "New phase",
    role: "Agent",
    provider: "ai-command",
    action: "claude_code",
    promptParameter: "prompt",
    profile: "claude",
    prompt: "",
    timeoutSeconds: 3600,
    actionParameters: {},
    workspace: true,
    retainEvidence: true,
    routeMode: "terminal",
    nextPhase: "",
    allowedNextPhases: [],
    responseTextPointer: "/response/result",
  });
}

function actionsFor(provider: string): ActionMetadata[] {
  return (
    props.providers
      .find((entry) => entry.name === provider)
      ?.actions.filter((action) => !action.pure) ?? []
  );
}

function editableParameters(phase: MissionPhaseDraft) {
  return (
    actionsFor(phase.provider)
      .find((entry) => entry.function_name === phase.action)
      ?.parameters.filter((parameter) => parameter.name !== phase.promptParameter) ?? []
  );
}

function selectProvider(phase: MissionPhaseDraft): void {
  phase.action = actionsFor(phase.provider)[0]?.function_name ?? "";
  selectAction(phase);
}

function selectAction(phase: MissionPhaseDraft): void {
  const action = actionsFor(phase.provider).find((entry) => entry.function_name === phase.action);

  if (action?.agent) {
    phase.promptParameter = action.agent.prompt_parameter;
    phase.responseTextPointer = action.agent.response_text_pointer;
  } else {
    phase.promptParameter = "";
  }

  phase.actionParameters = Object.fromEntries(
    (action?.parameters ?? [])
      .filter((parameter) => parameter.name !== phase.promptParameter)
      .filter((parameter) => parameter.default_value !== undefined)
      .map((parameter) => [parameter.name, parameter.default_value ?? null]),
  );
}

function otherPhases(id: string): MissionPhaseDraft[] {
  return draft.value.phases.filter((phase) => phase.id !== id);
}

function parameterText(phase: MissionPhaseDraft): string {
  return JSON.stringify(phase.actionParameters, null, 2);
}

function updateParameters(phase: MissionPhaseDraft, event: Event): void {
  try {
    const value = JSON.parse((event.target as HTMLTextAreaElement).value) as unknown;

    if (value && typeof value === "object" && !Array.isArray(value)) {
      phase.actionParameters = value as MissionPhaseDraft["actionParameters"];
    }
  } catch {
    /* review validation retains the last valid object. */
  }
}

function submit(): void {
  if (!issues.value.length) {
    emit("save", structuredClone(draft.value));
  }
}
</script>

<style scoped>
.recipe-steps {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.35rem;
}
.recipe-steps button {
  display: flex;
  justify-content: center;
  gap: 0.35rem;
  border: 0;
  border-radius: 0.45rem;
  padding: 0.55rem;
  background: var(--color-bg-subtle);
  color: var(--color-fg-muted);
}
.recipe-steps button.is-active {
  background: var(--color-accent-muted);
  color: var(--color-accent-text);
  font-weight: 700;
}
.recipe-form,
.recipe-section {
  display: grid;
  gap: 0.85rem;
}
.recipe-section {
  min-height: 30rem;
}
.recipe-section h3,
.recipe-section p {
  margin: 0;
}
.preset-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 0.65rem;
}
.preset-grid button {
  display: grid;
  gap: 0.3rem;
  text-align: left;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.65rem;
  padding: 0.8rem;
  background: var(--color-bg-subtle);
  color: var(--color-fg);
}
.preset-grid button.selected {
  border-color: var(--color-accent);
  background: var(--color-accent-muted);
}
.preset-grid span {
  color: var(--color-fg-muted);
  font-size: 0.75rem;
}
.section-heading,
.phase-card-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}
.input-card,
.phase-card {
  display: grid;
  gap: 0.7rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.65rem;
  padding: 0.75rem;
  background: var(--color-bg-subtle);
}
.check-field {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  padding-top: 1.5rem;
}
.route-targets {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.6rem;
}
.route-targets label {
  display: flex;
  gap: 0.3rem;
}
.review-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.65rem;
}
.review-grid div {
  padding: 0.7rem;
  border-radius: 0.5rem;
  background: var(--color-bg-subtle);
}
.review-grid dt {
  color: var(--color-fg-muted);
  font-size: 0.7rem;
}
.review-grid dd {
  margin: 0.2rem 0 0;
}
.error-list {
  color: var(--color-danger-text);
}
.success-note {
  color: var(--color-success-text);
}
@media (max-width: 700px) {
  .recipe-steps,
  .preset-grid,
  .review-grid {
    grid-template-columns: 1fr;
  }
}
</style>
