<template>
  <Modal
    title="Orchestration setup"
    width="min(820px, 100%)"
    :close-on-backdrop="false"
    @close="emit('close')"
  >
    <div class="orchestration-wizard-progress" aria-label="Setup progress">
      <button
        v-for="(item, index) in steps"
        :key="item"
        type="button"
        :class="{ 'is-active': step === index }"
        @click="step = index"
      >
        <span>{{ index + 1 }}</span
        >{{ item }}
      </button>
    </div>

    <form id="orchestration-setup-form" class="grid gap-4" @submit.prevent="save">
      <section v-if="step === 0" class="orchestration-wizard-section">
        <div class="flex items-center gap-1">
          <h3>Preset</h3>
          <HelpBubble label="About orchestration presets">
            Presets fill in a safe starting policy. Every generated field remains visible in the
            existing editor after saving.
          </HelpBubble>
        </div>
        <div class="orchestration-preset-grid">
          <div
            v-for="item in presets"
            :key="item.value"
            class="orchestration-preset"
            :class="{ 'is-selected': draft.preset === item.value }"
          >
            <button type="button" @click="selectPreset(item.value)">
              <Icon :name="item.icon" :size="18" />
              <strong>{{ item.label }}</strong>
            </button>
            <HelpBubble :label="`About ${item.label}`" :size="14">{{ item.help }}</HelpBubble>
          </div>
        </div>
        <label class="field-label">
          <span>Pipeline</span>
          <input class="input" :value="pipeline.name" disabled />
        </label>
      </section>

      <section v-else-if="step === 1" class="orchestration-wizard-section">
        <div class="flex items-center gap-1">
          <h3>Ingress</h3>
          <HelpBubble label="About ingress">
            <strong>Incoming events</strong><br />Choose how events enter this pipeline and which
            event begins correlated work.
          </HelpBubble>
        </div>
        <label class="field-label">
          <span>Event source</span>
          <select v-model="adapterId" class="input">
            <option value="">Manual or API</option>
            <option v-for="adapter in adapters" :key="adapter.id" :value="adapter.id">
              {{ adapter.name }} · {{ adapter.kind }}
            </option>
          </select>
        </label>
        <div class="grid gap-3 md:grid-cols-2">
          <label class="field-label">
            <span>Start event</span>
            <input v-model.trim="draft.startEvent" class="input" list="setup-events" required />
          </label>
          <label class="field-label">
            <span>Update event</span>
            <input v-model.trim="draft.updateEvent" class="input" list="setup-events" required />
          </label>
        </div>
        <details class="mission-advanced">
          <summary>Advanced identity</summary>
          <label class="field-label mt-2">
            <span>Correlation scope</span>
            <input v-model.trim="draft.scope" class="input" required />
          </label>
        </details>
        <datalist id="setup-events">
          <option v-for="event in availableEvents" :key="event" :value="event" />
        </datalist>
      </section>

      <section v-else-if="step === 2" class="orchestration-wizard-section">
        <div class="flex items-center gap-1">
          <h3>Behavior</h3>
          <HelpBubble label="About orchestration behavior">
            These choices compile into admission routes and intents. The active choice applies only
            while correlated work is running.
          </HelpBubble>
        </div>
        <label class="field-label">
          <span>While active</span>
          <select v-model="draft.activeBehavior" class="input">
            <option value="record">Record the event</option>
            <option value="queue">Queue it</option>
            <option value="restart">Restart with the latest event</option>
            <option value="signal">Signal active work</option>
          </select>
        </label>
        <label v-if="draft.activeBehavior === 'signal'" class="field-label">
          <span>Workflow signal</span>
          <input v-model.trim="draft.signalName" class="input" required />
        </label>
        <label class="field-label">
          <span>After completion</span>
          <select v-model="draft.terminalBehavior" class="input">
            <option value="ignore">Ignore later events</option>
            <option value="record">Record for audit</option>
            <option value="reopen">Open a new generation</option>
          </select>
        </label>
      </section>

      <section v-else-if="step === 3" class="orchestration-wizard-section">
        <div class="flex items-center gap-1">
          <h3>Execution</h3>
          <HelpBubble label="About orchestration execution">
            Select the entry phase, execution bound, failure budget, and whether phases reuse
            working files.
          </HelpBubble>
        </div>
        <div class="grid gap-3 md:grid-cols-2">
          <label class="field-label">
            <span>Entry phase</span>
            <select v-model="draft.entryMember" class="input" required>
              <option v-for="member in members" :key="member" :value="member">{{ member }}</option>
            </select>
          </label>
          <label class="field-label">
            <span>Maximum epochs</span>
            <input v-model.number="draft.maxEpochs" class="input" type="number" min="1" required />
          </label>
          <label class="field-label">
            <span>Retry attempts</span>
            <input v-model.number="draft.retryAttempts" class="input" type="number" min="0" />
          </label>
          <label class="field-label">
            <span>When exhausted</span>
            <select
              v-model="draft.retryExhaustion"
              class="input"
              :disabled="draft.retryAttempts === 0"
            >
              <option value="fail">Fail</option>
              <option value="pause">Pause</option>
              <option value="terminate">Terminate</option>
            </select>
          </label>
        </div>
        <label class="flex items-center gap-2 text-sm">
          <input v-model="draft.sharedWorkspace" type="checkbox" @change="toggleWorkspace" />
          Reuse a workspace across phases
        </label>
        <div v-if="draft.sharedWorkspace" class="grid gap-3 md:grid-cols-2">
          <label class="field-label">
            <span>Workspace scope</span>
            <input v-model.trim="draft.workspaceScope" class="input" required />
          </label>
          <label class="field-label">
            <span>Lease seconds</span>
            <input
              v-model.number="draft.workspaceLeaseSeconds"
              class="input"
              type="number"
              min="1"
              required
            />
          </label>
        </div>
        <div v-if="draft.preset === 'mission'" class="grid gap-3">
          <label class="field-label">
            <span>Mission kind</span>
            <select v-model="draft.missionKind" class="input" @change="applyMissionLimit">
              <option value="coding">Coding</option>
              <option value="research_report">Research/report</option>
            </select>
          </label>
          <div class="orchestration-phase-list">
            <article v-for="phase in draft.phases" :key="phase.member">
              <strong>{{ phase.member }}</strong>
              <label><input v-model="phase.terminal" type="checkbox" /> Terminal</label>
              <label><input v-model="phase.retainResults" type="checkbox" /> Save results</label>
              <label><input v-model="phase.workspace" type="checkbox" /> Workspace</label>
            </article>
          </div>
        </div>
      </section>

      <section v-else class="orchestration-wizard-section">
        <div class="flex items-center gap-1">
          <h3>Review</h3>
          <HelpBubble label="About the generated setup">
            Open Information to inspect the generated behavior, terminology, and policy before
            saving.
          </HelpBubble>
        </div>
        <dl class="orchestration-review-grid">
          <div>
            <dt>Preset</dt>
            <dd>{{ presetLabel }}</dd>
          </div>
          <div>
            <dt>Scope</dt>
            <dd>{{ draft.scope }}</dd>
          </div>
          <div>
            <dt>Entry</dt>
            <dd>{{ draft.entryMember }}</dd>
          </div>
          <div>
            <dt>Epoch limit</dt>
            <dd>{{ draft.maxEpochs }}</dd>
          </div>
          <div>
            <dt>Event source</dt>
            <dd>{{ selectedAdapter?.name ?? "Manual or API" }}</dd>
          </div>
          <div>
            <dt>Policy issues</dt>
            <dd>{{ validationIssues.length }}</dd>
          </div>
        </dl>
        <button type="button" class="btn w-fit" @click="infoOpen = true">
          <Icon name="info" :size="15" /> Information
        </button>
        <ul v-if="validationIssues.length" class="orchestration-errors-list">
          <li v-for="issue in validationIssues" :key="issue">{{ issue }}</li>
        </ul>
      </section>
    </form>

    <template #actions>
      <Button variant="ghost" @click="emit('close')">Cancel</Button>
      <Button v-if="step > 0" @click="step -= 1">Back</Button>
      <Button v-if="step < steps.length - 1" variant="primary" @click="step += 1">Next</Button>
      <Button
        v-else
        variant="primary"
        form="orchestration-setup-form"
        type="submit"
        :disabled="validationIssues.length > 0"
        :loading="saving"
      >
        Save setup
      </Button>
    </template>
  </Modal>

  <OrchestrationInfoView
    v-if="infoOpen"
    :draft="draft"
    :compiled="compiled"
    @close="infoOpen = false"
  />
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  AdapterDefinition,
  AdapterKindMetadata,
  JsonRecord,
  Pipeline,
} from "../../../core/domain/models";
import {
  applyPreset,
  classifyOrchestrationSetup,
  compileOrchestrationSetup,
  defaultSetupDraft,
  draftFromPipeline,
  mergeSetupMetadata,
  type OrchestrationSetupDraft,
  type OrchestrationSetupPreset,
} from "../../../core/services";
import type { IconName } from "../../../core/domain/icons";
import Button from "../shared/Button.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
import Modal from "../shared/Modal.vue";
import OrchestrationInfoView from "./OrchestrationInfoView.vue";

const props = withDefaults(
  defineProps<{
    pipeline: Pipeline;
    adapters?: AdapterDefinition[];
    adapterKinds?: AdapterKindMetadata[];
    saving?: boolean;
  }>(),
  { adapters: () => [], adapterKinds: () => [], saving: false },
);
const emit = defineEmits<{ close: []; save: [metadata: JsonRecord] }>();
const steps = ["Pattern", "Ingress", "Behavior", "Execution", "Review"];
const step = ref(0);
const infoOpen = ref(false);
const existingClassification = classifyOrchestrationSetup(props.pipeline);
const initialPreset =
  existingClassification && existingClassification !== "custom"
    ? existingClassification
    : "run_once";
const existingDraft =
  existingClassification === "custom" ? null : draftFromPipeline(props.pipeline, initialPreset);
const draft = ref<OrchestrationSetupDraft>(
  existingDraft ?? defaultSetupDraft(props.pipeline, initialPreset),
);
const adapterId = ref(draft.value.adapterIds[0] ?? "");
const members = computed(() => props.pipeline.graph.members.map((member) => member.key));
const selectedAdapter = computed(() =>
  props.adapters.find((adapter) => adapter.id === adapterId.value),
);
const selectedKind = computed(() =>
  props.adapterKinds.find((kind) => kind.kind === selectedAdapter.value?.kind),
);
const availableEvents = computed(() => selectedKind.value?.event_names ?? []);
const compiled = computed(() => compileOrchestrationSetup(draft.value));
const presetLabel = computed(
  () => presets.find((item) => item.value === draft.value.preset)?.label ?? draft.value.preset,
);
const validationIssues = computed(() => {
  const issues: string[] = [];

  if (!draft.value.scope.trim()) {
    issues.push("Correlation scope is required.");
  }

  if (!draft.value.startEvent.trim()) {
    issues.push("Start event is required.");
  }

  if (!draft.value.updateEvent.trim()) {
    issues.push("Update event is required.");
  }

  if (!draft.value.entryMember || !members.value.includes(draft.value.entryMember)) {
    issues.push("Choose an entry phase.");
  }

  if (draft.value.maxEpochs < 1) {
    issues.push("Maximum epochs must be at least one.");
  }

  if (draft.value.preset === "mission" && !draft.value.phases.some((phase) => phase.terminal)) {
    issues.push("A mission requires at least one terminal phase.");
  }

  return issues;
});

const presets: {
  value: OrchestrationSetupPreset;
  label: string;
  help: string;
  icon: IconName;
}[] = [
  {
    value: "run_once",
    label: "Run once",
    help: "Deduplicate repeated events for one correlation key.",
    icon: "runs",
  },
  {
    value: "queue_per_key",
    label: "Queue per key",
    help: "Wait to process repeated events until active work settles.",
    icon: "clock",
  },
  {
    value: "latest_wins",
    label: "Latest wins",
    help: "Replace active work when a newer event arrives.",
    icon: "refresh",
  },
  {
    value: "live_update",
    label: "Live update",
    help: "Send repeated events into the active workflow as a signal.",
    icon: "bolt",
  },
  {
    value: "pause_for_review",
    label: "Pause for review",
    help: "Pause when the configured failure budget is exhausted.",
    icon: "flag",
  },
  {
    value: "mission",
    label: "Mission",
    help: "Create a bounded, multi-phase orchestration with durable evidence.",
    icon: "branch",
  },
];

watch(adapterId, (value) => {
  draft.value.adapterIds = value ? [value] : [];
  const events = availableEvents.value;

  if (events.length) {
    draft.value.startEvent = events[0];
    draft.value.updateEvent = events[1] ?? events[0];
  }
});

function selectPreset(preset: OrchestrationSetupPreset): void {
  draft.value = applyPreset(props.pipeline, draft.value, preset);
  adapterId.value = draft.value.adapterIds[0] ?? "";
}

function toggleWorkspace(): void {
  for (const phase of draft.value.phases) {
    phase.workspace = draft.value.sharedWorkspace;
  }
}

function applyMissionLimit(): void {
  draft.value.maxEpochs = draft.value.missionKind === "research_report" ? 8 : 10;
}

function save(): void {
  if (validationIssues.value.length) {
    return;
  }

  emit("save", mergeSetupMetadata(props.pipeline.metadata, compiled.value));
}
</script>

<style scoped>
.orchestration-wizard-progress {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 0.3rem;
}
.orchestration-wizard-progress button {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.35rem;
  border: 0;
  border-radius: 0.45rem;
  padding: 0.5rem;
  background: var(--color-bg-subtle);
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.orchestration-wizard-progress button span {
  display: grid;
  width: 1.25rem;
  height: 1.25rem;
  place-items: center;
  border-radius: 999px;
  background: var(--color-bg);
}
.orchestration-wizard-progress button.is-active {
  background: var(--color-accent-muted);
  color: var(--color-accent-text);
  font-weight: 700;
}
.orchestration-wizard-section {
  display: grid;
  gap: 0.85rem;
  min-height: 25rem;
}
.orchestration-wizard-section h3 {
  margin: 0;
  font-size: 1rem;
}
.orchestration-preset-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.55rem;
}
.orchestration-preset {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  border: 1px solid var(--color-border);
  border-radius: 0.6rem;
  background: var(--color-bg-subtle);
  color: var(--color-fg);
  text-align: left;
}
.orchestration-preset > button {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  padding: 0.75rem;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
}
.orchestration-preset > .help-bubble {
  margin-right: 0.4rem;
}
.orchestration-preset.is-selected {
  border-color: var(--color-accent);
  background: var(--color-accent-muted);
}
.orchestration-phase-list {
  display: grid;
  gap: 0.4rem;
}
.orchestration-phase-list article {
  display: grid;
  grid-template-columns: minmax(0, 1fr) repeat(3, auto);
  gap: 0.7rem;
  align-items: center;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.45rem;
  padding: 0.55rem 0.65rem;
  font-size: 0.75rem;
}
.orchestration-phase-list label {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}
.orchestration-review-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.5rem;
  margin: 0;
}
.orchestration-review-grid div {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.45rem;
  padding: 0.65rem;
}
.orchestration-review-grid dt {
  color: var(--color-fg-muted);
  font-size: 0.68rem;
  text-transform: uppercase;
}
.orchestration-review-grid dd {
  margin: 0.2rem 0 0;
  overflow-wrap: anywhere;
  font-size: 0.82rem;
  font-weight: 600;
}
.orchestration-errors-list {
  margin: 0;
  padding: 0.7rem 0.7rem 0.7rem 1.8rem;
  border: 1px solid var(--color-danger);
  border-radius: 0.5rem;
  color: var(--color-danger-fg);
  font-size: 0.78rem;
}
@media (max-width: 680px) {
  .orchestration-wizard-progress,
  .orchestration-preset-grid,
  .orchestration-review-grid {
    grid-template-columns: 1fr;
  }
  .orchestration-wizard-progress button:not(.is-active) {
    display: none;
  }
  .orchestration-phase-list article {
    grid-template-columns: 1fr;
  }
}
</style>
