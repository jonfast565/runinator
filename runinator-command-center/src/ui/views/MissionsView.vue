<template>
  <section class="pane h-full overflow-auto">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-3">
      <div class="panel">
        <PanelHeader
          title="Missions"
          icon="branch"
          eyebrow="AI orchestration"
          description="Start bounded coding or research/report work through a durable mission pipeline. The graph stays immutable; only approved phase outcomes create a new epoch."
        >
          <button class="btn" :disabled="loading" @click="refresh">
            <LoadingSpinner v-if="loading" size="sm" label="Refreshing missions" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
          <button
            class="btn btn-primary"
            :disabled="!missionPipelines.length"
            @click="startOpen = true"
          >
            <Icon name="runs" />
            <span>New mission</span>
          </button>
        </PanelHeader>
        <p v-if="error" class="error m-0 text-sm">{{ error }}</p>
        <p v-else-if="!missionPipelines.length && !loading" class="m-0 text-sm text-fg-muted">
          No mission recipes are installed. Apply the AI missions pack, then return here to start
          one.
        </p>
      </div>

      <div class="grid min-h-0 gap-3 lg:grid-cols-[minmax(19rem,0.8fr)_minmax(28rem,1.2fr)]">
        <section class="panel min-h-0 p-0">
          <PanelHeader
            class="px-3 pt-3"
            title="Mission queue"
            icon="runs"
            heading="h3"
            description="Each mission is one correlated orchestration binding and may span several immutable execution epochs."
          />
          <EmptyState v-if="loading && !missions.length" compact loading title="Loading missions" />
          <EmptyState
            v-else-if="!missions.length"
            compact
            icon="branch"
            title="No missions yet"
            description="Start a recipe to create a durable mission."
          />
          <div v-else class="max-h-[55vh] overflow-auto p-2 lg:max-h-[calc(100vh-17rem)]">
            <button
              v-for="mission in missions"
              :key="mission.id"
              class="mission-row"
              :class="{ 'is-selected': mission.id === selectedId }"
              type="button"
              @click="select(mission.id)"
            >
              <span class="mission-state" :class="`is-${mission.status}`"></span>
              <span class="min-w-0 flex-1 text-left">
                <span class="block truncate font-medium text-fg">{{
                  mission.correlation_key
                }}</span>
                <span class="block truncate text-xs text-fg-muted">
                  {{ mission.scope }} · {{ mission.current_phase || "Waiting to start" }}
                </span>
              </span>
              <span class="text-xs capitalize text-fg-muted">{{ mission.status }}</span>
            </button>
          </div>
        </section>

        <section class="panel min-h-0">
          <EmptyState v-if="detailLoading" loading title="Loading mission detail" />
          <EmptyState
            v-else-if="!selected"
            icon="branch"
            title="Select a mission"
            description="Its current phase, evidence, and lifecycle controls will appear here."
          />
          <template v-else>
            <PanelHeader
              :title="selected.correlation_key"
              icon="branch"
              eyebrow="Mission detail"
              :description="`Frozen pipeline revision ${selected.pipeline_revision}; phase outcomes can only route to declared phases.`"
            >
              <span class="status-chip" :class="`is-${selected.status}`">{{
                selected.status
              }}</span>
            </PanelHeader>
            <dl class="mission-metrics">
              <div>
                <dt>Phase</dt>
                <dd>{{ selected.current_phase || "Admission" }}</dd>
              </div>
              <div>
                <dt>Epoch</dt>
                <dd>{{ selected.current_epoch }}</dd>
              </div>
              <div>
                <dt>Attempts</dt>
                <dd>{{ selected.current_attempt }}</dd>
              </div>
              <div>
                <dt>Updated</dt>
                <dd>{{ formatDate(selected.updated_at) }}</dd>
              </div>
            </dl>

            <div class="mt-4 grid gap-4 xl:grid-cols-2">
              <section>
                <div class="section-label">
                  <h3>Epochs</h3>
                  <span>{{ epochs.length }}</span>
                </div>
                <EmptyState
                  v-if="!epochs.length"
                  compact
                  icon="runs"
                  title="No epoch yet"
                  description="The reducer is preparing the admitted mission."
                />
                <ol v-else class="mission-events">
                  <li v-for="epoch in epochs" :key="epoch.id">
                    <span class="event-dot"></span>
                    <div>
                      <strong>Epoch {{ epoch.epoch }}</strong>
                      <p>{{ epoch.start_member || "Pipeline entry" }} · {{ epoch.status }}</p>
                      <small>{{ epoch.reason }}</small>
                    </div>
                  </li>
                </ol>
              </section>

              <section>
                <div class="section-label">
                  <h3>Evidence</h3>
                  <span>{{ evidence.length }}</span>
                </div>
                <EmptyState
                  v-if="!evidence.length"
                  compact
                  icon="box"
                  title="No evidence yet"
                  description="Phase results are retained here as they settle."
                />
                <ol v-else class="mission-events">
                  <li v-for="entry in evidence.slice(0, 8)" :key="entry.id">
                    <span class="event-dot"></span>
                    <div>
                      <strong>{{ entry.kind }}</strong>
                      <p>{{ entry.subject_revision || "No source revision" }}</p>
                      <small>{{ formatDate(entry.created_at) }}</small>
                    </div>
                  </li>
                </ol>
              </section>
            </div>

            <section class="mission-activity">
              <div class="section-label">
                <h3>Live harness activity</h3>
                <span>{{ effects.length }}</span>
              </div>
              <EmptyState
                v-if="!effects.length"
                compact
                icon="message"
                title="No steerable harness phase"
                description="Streaming Claude events appear here while a harnessed phase is active."
              />
              <div v-else class="grid gap-2 pt-3">
                <article v-for="activity in effects" :key="activity.effect.id" class="harness-card">
                  <div class="flex items-center justify-between gap-2">
                    <strong class="truncate">{{ activity.effect.id }}</strong>
                    <span class="status-chip" :class="`is-${activity.effect.status}`">
                      {{ activity.effect.status }}
                    </span>
                  </div>
                  <p v-if="activity.effect.message" class="m-0 mt-1 text-xs text-fg-muted">
                    {{ activity.effect.message }}
                  </p>
                  <ul v-if="activity.output.length" class="harness-events">
                    <li v-for="event in activity.output.slice(-4)" :key="event.event_id">
                      <template v-if="event.output.type === 'progress'">
                        {{ event.output.kind }}
                      </template>
                      <template v-else-if="event.output.type === 'chunk'">
                        {{ event.output.stream }}: {{ event.output.content }}
                      </template>
                      <template v-else>terminal interaction</template>
                    </li>
                  </ul>
                </article>
              </div>
            </section>

            <form class="mission-steering" @submit.prevent="submitSteering">
              <label>
                <span>Harness input</span>
                <textarea
                  v-model.trim="steeringMessage"
                  class="input min-h-20"
                  :disabled="!effects.length"
                  placeholder="Add context or redirect the active Claude Code phase…"
                ></textarea>
              </label>
              <button
                class="btn"
                :disabled="steering || !effects.length || !steeringMessage"
                type="submit"
              >
                <LoadingSpinner v-if="steering" size="sm" label="Sending harness input" />
                <Icon v-else name="message" />
                <span>Steer harness</span>
              </button>
            </form>

            <form class="mission-intent" @submit.prevent="submitIntent">
              <label>
                <span>Lifecycle intent</span>
                <input
                  v-model.trim="intentName"
                  class="input"
                  placeholder="pause, resume, cancel…"
                  required
                />
              </label>
              <label>
                <span>Reason</span>
                <input
                  v-model.trim="intentReason"
                  class="input"
                  placeholder="Why this action is needed"
                  required
                />
              </label>
              <button
                class="btn"
                :disabled="intentSubmitting || !intentName || !intentReason"
                type="submit"
              >
                <LoadingSpinner v-if="intentSubmitting" size="sm" label="Sending mission intent" />
                <Icon v-else name="flag" />
                <span>Send intent</span>
              </button>
            </form>
            <p class="mt-2 mb-0 text-xs text-fg-muted">
              Only intents authored by this mission recipe are accepted. Harness input becomes an
              ordered Claude user message and is retained in the effect event stream.
            </p>
          </template>
        </section>
      </div>
    </div>
  </section>

  <Modal
    v-if="startOpen"
    title="Start mission"
    description="Mission input is recorded in the ingress ledger before any phase can begin."
    @close="startOpen = false"
  >
    <form id="start-mission" class="grid gap-3" @submit.prevent="submitStart">
      <label class="field-label">
        <span>Recipe</span>
        <select v-model="startPipelineId" class="input" required>
          <option value="" disabled>Choose a mission recipe…</option>
          <option
            v-for="pipeline in missionPipelines"
            :key="pipeline.id ?? ''"
            :value="pipeline.id ?? ''"
          >
            {{ pipeline.name }}
          </option>
        </select>
      </label>
      <label class="field-label">
        <span>Mission type</span>
        <output class="input flex items-center bg-surface-subtle text-fg-muted">
          {{ startKind === "coding" ? "Coding loop" : "Coding research/report loop" }}
        </output>
      </label>
      <label class="field-label">
        <span>Correlation key</span>
        <input
          v-model.trim="startCorrelation"
          class="input"
          placeholder="feature-auth-refresh"
          required
        />
      </label>
      <label class="field-label">
        <span>Mission input JSON</span>
        <textarea
          v-model="startParameters"
          class="input min-h-40 font-mono text-xs"
          spellcheck="false"
        ></textarea>
      </label>
      <p v-if="startError" class="error m-0 text-sm">{{ startError }}</p>
    </form>
    <template #footer>
      <button class="btn" type="button" @click="startOpen = false">Cancel</button>
      <button class="btn btn-primary" form="start-mission" :disabled="starting" type="submit">
        <LoadingSpinner v-if="starting" size="sm" label="Starting mission" />
        <Icon v-else name="runs" />
        <span>Start mission</span>
      </button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { JsonRecord } from "../../core/domain/models";
import { asJsonRecord } from "../../core/domain/json";
import type { MissionKind } from "../../core/services";
import { useMissionsStore } from "../adapters/pinia/missions";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import Modal from "../components/shared/Modal.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";

const missionsStore = useMissionsStore();
const {
  missionPipelines,
  missions,
  selectedId,
  selected,
  epochs,
  evidence,
  effects,
  loading,
  detailLoading,
  error,
} = storeToRefs(missionsStore);
const startOpen = ref(false);
const starting = ref(false);
const startError = ref<string | null>(null);
const startPipelineId = ref("");
const startCorrelation = ref("");
const startParameters = ref(
  '{\n  "request": {\n    "goal": ""\n  },\n  "mission": {\n    "source": {\n      "repository": "",\n      "revision": ""\n    }\n  }\n}',
);
const intentName = ref("");
const intentReason = ref("");
const intentSubmitting = ref(false);
const steeringMessage = ref("");
const steering = ref(false);
let refreshTimer = 0;

const selectedPipeline = computed(() =>
  missionPipelines.value.find((pipeline) => pipeline.id === startPipelineId.value),
);
const startKind = computed<MissionKind>(() => {
  const ingress = asJsonRecord(selectedPipeline.value?.metadata.ingress);
  return ingress.scope === "mission.research_report" ? "research_report" : "coding";
});

async function refresh(): Promise<void> {
  await missionsStore.refresh();

  if (
    !startPipelineId.value ||
    !missionPipelines.value.some((pipeline) => pipeline.id === startPipelineId.value)
  ) {
    startPipelineId.value = missionPipelines.value[0]?.id ?? "";
  }
}

async function select(id: string): Promise<void> {
  await missionsStore.select(id);
}

async function submitSteering(): Promise<void> {
  const activity = effects.value.at(0);

  if (!activity || !steeringMessage.value) {
    return;
  }

  steering.value = true;
  error.value = null;

  try {
    const missionId = selectedId.value;

    if (!missionId) {
      return;
    }

    await missionsStore.steer(missionId, steeringMessage.value);
    steeringMessage.value = "";
    await select(selectedId.value ?? "");
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    steering.value = false;
  }
}

async function submitStart(): Promise<void> {
  startError.value = null;
  let parameters: JsonRecord;

  try {
    const parsed: unknown = JSON.parse(startParameters.value);

    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("Mission input must be a JSON object.");
    }

    parameters = parsed as JsonRecord;
  } catch (cause) {
    startError.value = cause instanceof Error ? cause.message : "Mission input is not valid JSON.";
    return;
  }

  const pipelineId = selectedPipeline.value?.id;

  if (!pipelineId || !startCorrelation.value) {
    return;
  }

  starting.value = true;

  try {
    const response = await missionsStore.start({
      pipelineId,
      kind: startKind.value,
      correlationKey: startCorrelation.value,
      parameters,
    });
    startOpen.value = false;

    if (response.orchestration_binding_id) {
      selectedId.value = response.orchestration_binding_id;
    }

    await refresh();
  } catch (cause) {
    startError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    starting.value = false;
  }
}

async function submitIntent(): Promise<void> {
  if (!selected.value || !intentName.value || !intentReason.value) {
    return;
  }

  intentSubmitting.value = true;
  error.value = null;

  try {
    await missionsStore.intent(selected.value.id, intentName.value, intentReason.value);
    intentName.value = "";
    intentReason.value = "";
    await refresh();
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    intentSubmitting.value = false;
  }
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

onMounted(() => {
  void refresh();
  refreshTimer = window.setInterval(() => void refresh(), 15_000);
});

onBeforeUnmount(() => {
  window.clearInterval(refreshTimer);
});
</script>

<style scoped>
.mission-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 0.6rem;
  border: 1px solid transparent;
  border-radius: 0.55rem;
  padding: 0.65rem;
  color: inherit;
  background: transparent;
}
.mission-row:hover,
.mission-row.is-selected {
  border-color: color-mix(in srgb, var(--color-accent) 35%, transparent);
  background: var(--color-accent-soft);
}
.mission-state,
.event-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 999px;
  background: var(--color-fg-muted);
  flex: none;
}
.mission-state.is-running,
.mission-state.is-pending {
  background: var(--color-accent);
}
.mission-state.is-completed {
  background: var(--color-success-fg);
}
.mission-state.is-failed,
.mission-state.is-terminated {
  background: var(--color-danger-fg);
}
.mission-state.is-waiting,
.mission-state.is-suspended {
  background: var(--color-warning-fg);
}
.status-chip {
  border-radius: 999px;
  padding: 0.2rem 0.55rem;
  background: var(--color-bg-subtle);
  color: var(--color-fg-muted);
  font-size: 0.75rem;
  text-transform: capitalize;
}
.status-chip.is-running,
.status-chip.is-pending {
  background: var(--color-accent-soft);
  color: var(--color-accent-text);
}
.status-chip.is-completed {
  background: var(--color-success-bg);
  color: var(--color-success-fg);
}
.status-chip.is-failed,
.status-chip.is-terminated {
  background: var(--color-danger-bg);
  color: var(--color-danger-fg);
}
.mission-metrics {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.5rem;
  margin: 1rem 0 0;
}
.mission-metrics div {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.55rem;
  padding: 0.55rem 0.65rem;
}
.mission-metrics dt {
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.mission-metrics dd {
  margin: 0.15rem 0 0;
  color: var(--color-fg);
  font-size: 0.85rem;
  overflow-wrap: anywhere;
}
.section-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--color-border-subtle);
  padding-bottom: 0.4rem;
}
.section-label h3 {
  margin: 0;
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.section-label span {
  color: var(--color-fg-muted);
  font-size: 0.75rem;
}
.mission-events {
  margin: 0.65rem 0 0;
  padding: 0;
  list-style: none;
}
.mission-events li {
  display: grid;
  grid-template-columns: 0.7rem minmax(0, 1fr);
  gap: 0.55rem;
  padding: 0.3rem 0 0.5rem;
}
.mission-events p,
.mission-events small {
  display: block;
  margin: 0.15rem 0 0;
  color: var(--color-fg-muted);
  font-size: 0.75rem;
  overflow-wrap: anywhere;
}
.mission-activity {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--color-border-subtle);
}
.harness-card {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.55rem;
  padding: 0.6rem 0.7rem;
}
.harness-card strong {
  font-family: var(--font-mono);
  font-size: 0.72rem;
}
.harness-events {
  display: grid;
  gap: 0.2rem;
  margin: 0.55rem 0 0;
  padding: 0;
  color: var(--color-fg-muted);
  font-family: var(--font-mono);
  font-size: 0.7rem;
  list-style: none;
}
.harness-events li {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mission-steering {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 0.55rem;
  align-items: end;
  margin-top: 1rem;
}
.mission-steering label {
  display: grid;
  gap: 0.3rem;
  color: var(--color-fg-muted);
  font-size: 0.75rem;
}
.mission-intent {
  display: grid;
  grid-template-columns: minmax(9rem, 0.75fr) minmax(12rem, 1.5fr) auto;
  gap: 0.55rem;
  align-items: end;
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--color-border-subtle);
}
.mission-intent label,
.field-label {
  display: grid;
  gap: 0.3rem;
  color: var(--color-fg-muted);
  font-size: 0.75rem;
}
@media (max-width: 700px) {
  .mission-steering,
  .mission-intent {
    grid-template-columns: 1fr;
  }
}
</style>
