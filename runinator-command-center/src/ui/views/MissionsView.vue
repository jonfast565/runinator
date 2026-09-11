<template>
  <section class="pane h-full overflow-auto">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-3">
      <div class="panel">
        <PanelHeader
          title="Missions"
          icon="branch"
          eyebrow="AI orchestration"
          description="Launch bounded coding and research work, then follow its durable phases, evidence, and operator controls."
        >
          <Button :loading="loading" icon="refresh" @click="refresh">Refresh</Button>
          <Button
            variant="primary"
            icon="runs"
            :disabled="!missionPipelines.length"
            @click="startOpen = true"
          >
            New mission
          </Button>
        </PanelHeader>
        <p v-if="error" class="mission-page-error">{{ error }}</p>
        <p v-else-if="!missionPipelines.length && !loading" class="mission-page-notice">
          No mission recipes are installed. Apply the AI missions pack, then return here to launch
          one.
        </p>
      </div>

      <section v-if="missions.length" class="mission-summary" aria-label="Mission summary">
        <article>
          <span>Active</span>
          <strong>{{ missionCounts.active }}</strong>
          <small>currently progressing</small>
        </article>
        <article>
          <span>Needs attention</span>
          <strong>{{ missionCounts.attention }}</strong>
          <small>waiting, paused, or failed</small>
        </article>
        <article>
          <span>Complete</span>
          <strong>{{ missionCounts.completed }}</strong>
          <small>finished successfully</small>
        </article>
        <article>
          <span>All missions</span>
          <strong>{{ missionCounts.all }}</strong>
          <small>durable bindings</small>
        </article>
      </section>

      <div class="grid min-h-0 gap-3 lg:grid-cols-[minmax(19rem,0.8fr)_minmax(28rem,1.2fr)]">
        <MissionQueuePanel
          v-model:filter="queueFilter"
          v-model:search="queueSearch"
          :missions="visibleMissions"
          :total="missions.length"
          :selected-id="selectedId"
          :loading="loading"
          :counts="missionCounts"
          @refresh="refresh"
          @select="select"
        />

        <section class="panel min-h-0">
          <EmptyState v-if="detailLoading" loading title="Loading mission detail" />
          <EmptyState
            v-else-if="!selected"
            icon="branch"
            title="Select a mission"
            description="Its current phase, recorded evidence, and available controls will appear here."
          />
          <template v-else>
            <PanelHeader
              :title="selected.correlation_key"
              icon="branch"
              eyebrow="Mission detail"
              :description="missionStatusSummary(selected.status)"
            >
              <span class="status-chip" :class="`is-${selected.status}`">{{
                selected.status
              }}</span>
            </PanelHeader>

            <p class="mission-detail-description">
              {{ missionKindLabel(selected.scope) }} mission · frozen pipeline revision
              {{ selected.pipeline_revision }} · {{ selected.policy.max_epochs ?? "—" }} epoch limit
            </p>

            <dl class="mission-metrics">
              <div>
                <dt>Current phase</dt>
                <dd>{{ phaseLabel(selected.current_phase) }}</dd>
              </div>
              <div>
                <dt>Epoch progress</dt>
                <dd>{{ epochProgress(selected) }}</dd>
              </div>
              <div>
                <dt>Attempt</dt>
                <dd>{{ selected.current_attempt || "—" }}</dd>
              </div>
              <div>
                <dt>Last activity</dt>
                <dd :title="formatDate(selected.updated_at)">
                  {{ relativeDate(selected.updated_at) }}
                </dd>
              </div>
            </dl>

            <div class="mission-detail-grid">
              <section class="mission-detail-section">
                <div class="section-label">
                  <h3>Timeline</h3>
                  <span>{{ epochs.length }} epoch{{ epochs.length === 1 ? "" : "s" }}</span>
                </div>
                <EmptyState
                  v-if="!epochs.length"
                  compact
                  icon="runs"
                  title="No epoch yet"
                  description="The mission is preparing its admission."
                />
                <ol v-else class="mission-events">
                  <li
                    v-for="epoch in epochs"
                    :key="epoch.id"
                    :class="{ 'is-current': epoch.epoch === selected.current_epoch }"
                  >
                    <span class="event-dot" :class="`is-${epoch.status}`"></span>
                    <div class="min-w-0">
                      <div class="mission-event-heading">
                        <strong>Epoch {{ epoch.epoch }}</strong>
                        <span>{{ epoch.status }}</span>
                      </div>
                      <p>{{ phaseLabel(epoch.start_member) }}</p>
                      <small>{{ epoch.reason || "Mission phase recorded" }}</small>
                    </div>
                  </li>
                </ol>
              </section>

              <section class="mission-detail-section">
                <div class="section-label">
                  <h3>Evidence</h3>
                  <span>{{ evidence.length }} record{{ evidence.length === 1 ? "" : "s" }}</span>
                </div>
                <EmptyState
                  v-if="!evidence.length"
                  compact
                  icon="box"
                  title="No evidence yet"
                  description="Settled phase results are retained here."
                />
                <ol v-else class="mission-events mission-evidence-list">
                  <li v-for="entry in evidence.slice(0, 8)" :key="entry.id">
                    <span class="event-dot is-evidence"></span>
                    <div class="min-w-0">
                      <div class="mission-event-heading">
                        <strong>{{ evidenceLabel(entry.kind) }}</strong>
                        <span :title="formatDate(entry.created_at)">{{
                          relativeDate(entry.created_at)
                        }}</span>
                      </div>
                      <p>{{ evidenceSummary(entry) }}</p>
                      <small v-if="entry.subject_revision">{{ entry.subject_revision }}</small>
                    </div>
                  </li>
                </ol>
              </section>
            </div>

            <section class="mission-activity">
              <div class="section-label">
                <div>
                  <h3>Live harness</h3>
                  <p v-if="effects.length" class="mission-section-description">
                    Send context to the active Claude phase without leaving the mission.
                  </p>
                </div>
                <span>{{ effects.length ? "Active" : "Idle" }}</span>
              </div>
              <EmptyState
                v-if="!effects.length"
                compact
                icon="message"
                title="No active harness"
                description="Live output and operator steering appear when a harnessed phase is running."
              />
              <div v-else class="grid gap-2 pt-3">
                <article v-for="activity in effects" :key="activity.effect.id" class="harness-card">
                  <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                      <p class="harness-kicker">Current effect</p>
                      <strong :title="activity.effect.id">{{ shortId(activity.effect.id) }}</strong>
                    </div>
                    <span class="status-chip" :class="`is-${activity.effect.status}`">
                      {{ activity.effect.status }}
                    </span>
                  </div>
                  <p v-if="activity.effect.message" class="m-0 mt-2 text-xs text-fg-muted">
                    {{ activity.effect.message }}
                  </p>
                  <ul v-if="activity.output.length" class="harness-events">
                    <li v-for="event in activity.output.slice(-4)" :key="event.event_id">
                      {{ outputSummary(event) }}
                    </li>
                  </ul>
                </article>

                <form class="mission-steering" @submit.prevent="submitSteering">
                  <label>
                    <span>Add context for the active phase</span>
                    <textarea
                      v-model.trim="steeringMessage"
                      class="input min-h-20"
                      placeholder="Clarify priorities, point to an edge case, or redirect the investigation…"
                    ></textarea>
                  </label>
                  <Button
                    type="submit"
                    icon="message"
                    :loading="steering"
                    :disabled="!steeringMessage"
                  >
                    Send context
                  </Button>
                </form>
              </div>
            </section>

            <section v-if="declaredIntents.length" class="mission-controls">
              <div class="section-label">
                <div>
                  <h3>Lifecycle controls</h3>
                  <p class="mission-section-description">
                    These are the policy-defined actions available for this mission.
                  </p>
                </div>
              </div>
              <form class="mission-intent" @submit.prevent="submitIntent">
                <div class="intent-options" aria-label="Choose a lifecycle action">
                  <button
                    v-for="intent in declaredIntents"
                    :key="intent"
                    class="intent-option"
                    :class="{
                      'is-selected': intentName === intent,
                      'is-danger': intent === 'cancel' || intent === 'terminate',
                    }"
                    type="button"
                    :aria-pressed="intentName === intent"
                    @click="intentName = intent"
                  >
                    {{ intentLabel(intent) }}
                  </button>
                </div>
                <label>
                  <span>Reason</span>
                  <input
                    v-model.trim="intentReason"
                    class="input"
                    :placeholder="
                      intentName
                        ? `Why ${intentLabel(intentName).toLowerCase()} is needed`
                        : 'Choose an action first'
                    "
                    :disabled="!intentName"
                    required
                  />
                </label>
                <Button
                  type="submit"
                  icon="flag"
                  :loading="intentSubmitting"
                  :disabled="!intentName || !intentReason"
                  :variant="
                    intentName === 'cancel' || intentName === 'terminate' ? 'danger' : 'default'
                  "
                >
                  {{ intentName ? `Send ${intentLabel(intentName)}` : "Send action" }}
                </Button>
              </form>
            </section>
          </template>
        </section>
      </div>
    </div>
  </section>

  <MissionStartDialog
    v-if="startOpen"
    :pipelines="missionPipelines"
    :starting="starting"
    :error="startError"
    @close="startOpen = false"
    @start="submitStart"
  />
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { asJsonRecord } from "../../core/domain/json";
import type {
  OrchestrationBinding,
  OrchestrationEvidence,
  WorkflowEffectOutputEvent,
} from "../../core/domain/models";
import type { StartMissionInput } from "../../core/services";
import { useMissionsStore } from "../adapters/pinia/missions";
import MissionQueuePanel from "../components/missions/MissionQueuePanel.vue";
import MissionStartDialog from "../components/missions/MissionStartDialog.vue";
import Button from "../components/shared/Button.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";

type MissionQueueFilter = "all" | "active" | "attention" | "completed";

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
const queueFilter = ref<MissionQueueFilter>("all");
const queueSearch = ref("");
const intentName = ref("");
const intentReason = ref("");
const intentSubmitting = ref(false);
const steeringMessage = ref("");
const steering = ref(false);
let refreshTimer = 0;

const missionCounts = computed<Record<MissionQueueFilter, number>>(() => ({
  all: missions.value.length,
  active: missions.value.filter((mission) => isActive(mission.status)).length,
  attention: missions.value.filter((mission) => needsAttention(mission.status)).length,
  completed: missions.value.filter((mission) => mission.status === "completed").length,
}));
const visibleMissions = computed(() => {
  const query = queueSearch.value.trim().toLocaleLowerCase();

  return [...missions.value]
    .filter((mission) => matchesFilter(mission, queueFilter.value))
    .filter((mission) => !query || searchableMissionText(mission).includes(query))
    .sort((left, right) => right.updated_at.localeCompare(left.updated_at));
});
const declaredIntents = computed(() =>
  Object.entries(selected.value?.policy.intents ?? {})
    .sort(([, left], [, right]) => right.priority - left.priority)
    .map(([name]) => name),
);

watch(selectedId, () => {
  intentName.value = "";
  intentReason.value = "";
  steeringMessage.value = "";
});

async function refresh(): Promise<void> {
  await missionsStore.refresh();
}

async function select(id: string): Promise<void> {
  await missionsStore.select(id);
}

async function submitSteering(): Promise<void> {
  if (!selectedId.value || !steeringMessage.value) {
    return;
  }

  steering.value = true;
  error.value = null;

  try {
    await missionsStore.steer(selectedId.value, steeringMessage.value);
    steeringMessage.value = "";
    await select(selectedId.value);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    steering.value = false;
  }
}

async function submitStart(input: StartMissionInput): Promise<void> {
  startError.value = null;
  starting.value = true;

  try {
    const response = await missionsStore.start(input);
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

function isActive(status: string): boolean {
  return status === "pending" || status === "running";
}

function needsAttention(status: string): boolean {
  return (
    status === "waiting" || status === "suspended" || status === "failed" || status === "terminated"
  );
}

function matchesFilter(mission: OrchestrationBinding, filter: MissionQueueFilter): boolean {
  if (filter === "all") {
    return true;
  }

  if (filter === "active") {
    return isActive(mission.status);
  }

  if (filter === "attention") {
    return needsAttention(mission.status);
  }

  return mission.status === "completed";
}

function searchableMissionText(mission: OrchestrationBinding): string {
  return `${mission.correlation_key} ${mission.scope} ${mission.current_phase ?? ""}`.toLocaleLowerCase();
}

function missionKindLabel(scope: string): string {
  return scope === "mission.research_report" ? "Research/report" : "Coding";
}

function missionStatusSummary(status: string): string {
  switch (status) {
    case "running":
      return "The mission is actively progressing through its frozen pipeline.";
    case "pending":
      return "The mission has been admitted and is waiting for its first phase to start.";
    case "waiting":
      return "The mission is waiting for a durable phase transition or external dependency.";
    case "suspended":
      return "The mission is paused. Resume it when the work may continue.";
    case "completed":
      return "The mission completed. Review its evidence and final handoff.";
    case "failed":
      return "The mission stopped with a failure. Inspect its latest epoch and evidence.";
    case "terminated":
      return "The mission was terminated by its lifecycle policy.";
    default:
      return "This mission follows a frozen, durable phase policy.";
  }
}

function phaseLabel(phase: string | null | undefined): string {
  if (!phase) {
    return "Preparing to start";
  }

  return phase
    .replace(/^runinator\.missions\./, "")
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function epochProgress(mission: OrchestrationBinding): string {
  const limit = mission.policy.max_epochs;
  return limit
    ? `${String(mission.current_epoch)} of ${String(limit)}`
    : `Epoch ${String(mission.current_epoch)}`;
}

function evidenceLabel(kind: string): string {
  return kind.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

function evidenceSummary(entry: OrchestrationEvidence): string {
  const payload = asJsonRecord(entry.payload);
  const response = asJsonRecord(payload.response);
  const critique = asJsonRecord(payload.critique);
  const candidates = [payload.summary, response.result, critique.summary, payload.role];
  const summary = candidates.find(
    (candidate): candidate is string =>
      typeof candidate === "string" && candidate.trim().length > 0,
  );
  return summary ? truncate(summary, 130) : "Phase evidence recorded";
}

function outputSummary(event: WorkflowEffectOutputEvent): string {
  switch (event.output.type) {
    case "chunk":
      return `${event.output.stream}: ${truncate(event.output.content, 170)}`;
    case "progress":
      return event.output.kind;
    case "artifact":
      return "Artifact produced";
    case "terminal_interaction":
      return event.output.interaction.state === "input_required"
        ? "Terminal input required"
        : "Terminal input accepted";
  }
}

function intentLabel(intent: string): string {
  return intent.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-8)}` : value;
}

function truncate(value: string, maximum: number): string {
  return value.length > maximum ? `${value.slice(0, maximum - 1)}…` : value;
}

function relativeDate(value: string): string {
  const timestamp = new Date(value).getTime();

  if (Number.isNaN(timestamp)) {
    return value;
  }

  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));

  if (seconds < 60) {
    return "Just now";
  }

  const minutes = Math.floor(seconds / 60);

  if (minutes < 60) {
    return `${String(minutes)}m ago`;
  }

  const hours = Math.floor(minutes / 60);

  if (hours < 24) {
    return `${String(hours)}h ago`;
  }

  return `${String(Math.floor(hours / 24))}d ago`;
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
.mission-page-error,
.mission-page-notice {
  margin: 0.8rem 0 0;
  font-size: 0.8rem;
}
.mission-page-error {
  border: 1px solid var(--color-danger-fg);
  border-radius: 0.55rem;
  padding: 0.6rem 0.7rem;
  color: var(--color-danger-fg);
  background: var(--color-danger-bg);
}
.mission-page-notice,
.mission-detail-description,
.mission-section-description {
  color: var(--color-fg-muted);
}
.mission-summary {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.6rem;
}
.mission-summary article {
  display: grid;
  gap: 0.2rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.65rem;
  padding: 0.7rem 0.8rem;
  background: var(--color-bg-subtle);
}
.mission-summary span,
.mission-summary small {
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.mission-summary strong {
  color: var(--color-fg);
  font-size: 1.2rem;
  line-height: 1.1;
}
.status-chip {
  border-radius: 999px;
  padding: 0.2rem 0.55rem;
  color: var(--color-fg-muted);
  background: var(--color-bg-subtle);
  font-size: 0.75rem;
  text-transform: capitalize;
}
.status-chip.is-running,
.status-chip.is-pending {
  color: var(--color-accent-text);
  background: var(--color-accent-soft);
}
.status-chip.is-completed,
.status-chip.is-succeeded {
  color: var(--color-success-fg);
  background: var(--color-success-bg);
}
.status-chip.is-failed,
.status-chip.is-terminated,
.status-chip.is-rejected,
.status-chip.is-timed_out,
.status-chip.is-canceled {
  color: var(--color-danger-fg);
  background: var(--color-danger-bg);
}
.status-chip.is-waiting,
.status-chip.is-suspended {
  color: var(--color-warning-fg);
  background: var(--color-warning-bg);
}
.mission-detail-description {
  margin: 0.75rem 0 0;
  font-size: 0.75rem;
}
.mission-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
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
  font-size: 0.7rem;
}
.mission-metrics dd {
  margin: 0.15rem 0 0;
  overflow-wrap: anywhere;
  color: var(--color-fg);
  font-size: 0.82rem;
  font-weight: 600;
}
.mission-detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1rem;
  margin-top: 1rem;
}
.mission-detail-section {
  min-width: 0;
}
.section-label {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
  border-bottom: 1px solid var(--color-border-subtle);
  padding-bottom: 0.45rem;
}
.section-label h3 {
  margin: 0;
  color: var(--color-fg);
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.section-label > span {
  flex: none;
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.mission-section-description {
  margin: 0.2rem 0 0;
  font-size: 0.72rem;
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
  padding: 0.35rem 0 0.55rem;
}
.mission-events li.is-current {
  margin: 0 -0.35rem;
  border-radius: 0.45rem;
  padding: 0.35rem;
  background: var(--color-accent-soft);
}
.event-dot {
  width: 0.5rem;
  height: 0.5rem;
  margin-top: 0.3rem;
  border-radius: 999px;
  background: var(--color-fg-muted);
}
.event-dot.is-running,
.event-dot.is-pending {
  background: var(--color-accent);
}
.event-dot.is-succeeded,
.event-dot.is-completed,
.event-dot.is-evidence {
  background: var(--color-success-fg);
}
.event-dot.is-failed,
.event-dot.is-terminated {
  background: var(--color-danger-fg);
}
.mission-event-heading {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.5rem;
}
.mission-event-heading strong {
  overflow: hidden;
  color: var(--color-fg);
  font-size: 0.78rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mission-event-heading span,
.mission-events p,
.mission-events small {
  display: block;
  margin: 0.15rem 0 0;
  overflow-wrap: anywhere;
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.mission-event-heading span {
  flex: none;
  margin: 0;
}
.mission-activity,
.mission-controls {
  margin-top: 1rem;
  border-top: 1px solid var(--color-border-subtle);
  padding-top: 1rem;
}
.harness-card {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.55rem;
  padding: 0.7rem;
}
.harness-kicker {
  margin: 0;
  color: var(--color-fg-muted);
  font-size: 0.68rem;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.harness-card strong {
  color: var(--color-fg);
  font-family: var(--font-mono);
  font-size: 0.75rem;
}
.harness-events {
  display: grid;
  gap: 0.25rem;
  margin: 0.6rem 0 0;
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
  margin-top: 0.2rem;
}
.mission-steering label,
.mission-intent label {
  display: grid;
  gap: 0.3rem;
  color: var(--color-fg-muted);
  font-size: 0.75rem;
}
.mission-intent {
  display: grid;
  grid-template-columns: minmax(13rem, 1fr) minmax(13rem, 1.25fr) auto;
  gap: 0.55rem;
  align-items: end;
  margin-top: 0.8rem;
}
.intent-options {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  min-height: 2.25rem;
  align-content: end;
}
.intent-option {
  border: 1px solid var(--color-border-subtle);
  border-radius: 999px;
  padding: 0.35rem 0.55rem;
  color: var(--color-fg-muted);
  background: var(--color-bg-subtle);
  font-size: 0.75rem;
}
.intent-option:hover,
.intent-option.is-selected {
  border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
  color: var(--color-accent-text);
  background: var(--color-accent-soft);
}
.intent-option.is-danger:hover,
.intent-option.is-danger.is-selected {
  border-color: color-mix(in srgb, var(--color-danger-fg) 40%, transparent);
  color: var(--color-danger-fg);
  background: var(--color-danger-bg);
}
@media (max-width: 880px) {
  .mission-summary,
  .mission-metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .mission-intent {
    grid-template-columns: 1fr;
  }
}
@media (max-width: 640px) {
  .mission-detail-grid,
  .mission-summary,
  .mission-metrics,
  .mission-steering {
    grid-template-columns: 1fr;
  }
}
</style>
