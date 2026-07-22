<template>
  <section class="pane runs-pane">
    <SplitPane
      class="runs-layout"
      storage-key="command-center.pipeline-runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="!!store.selectedRunId"
    >
      <template #first>
        <div class="panel runs-list-panel">
          <div class="panel-toolbar">
            <div class="runs-copy">
              <h2>Pipeline Runs</h2>
              <p>First-class pipeline executions and the member workflow runs they orchestrate.</p>
            </div>
            <button class="btn" :disabled="store.loading" @click="store.refresh">
              <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing pipeline runs" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </div>
          <div class="runs-start">
            <select v-model="selectedPipelineId" class="input">
              <option value="">Start a pipeline…</option>
              <option v-for="pipeline in store.pipelines" :key="pipeline.id ?? ''" :value="pipeline.id ?? ''">
                {{ pipeline.name }}
              </option>
            </select>
            <button class="btn btn-primary" :disabled="!selectedPipelineId || starting" @click="startRun">
              <Icon name="runs" />
              <span>Start run</span>
            </button>
          </div>
          <div class="runs-summary">
            <div>
              <span>Visible</span>
              <strong>{{ store.runs.length }}</strong>
            </div>
            <div>
              <span>Active</span>
              <strong>{{ activeRunCount }}</strong>
            </div>
            <div>
              <span>Selected</span>
              <strong>{{ selectedRunLabel }}</strong>
            </div>
          </div>
          <p v-if="store.error" class="error-hint">{{ store.error }}</p>
          <EmptyState
            v-if="store.loading && !store.runs.length"
            compact
            loading
            title="Loading pipeline runs"
          />
          <EmptyState
            v-else-if="!store.runs.length"
            compact
            icon="runs"
            title="No pipeline runs yet"
            description="Start a pipeline above, or trigger one via a cron/chained pipeline trigger."
          />
          <div v-else class="table-scroll runs-table-scroll" :class="{ 'is-refreshing': store.loading }">
            <RunTable
              :runs="runRows"
              :selected-run-id="store.selectedRunId"
              :workflow-names="pipelineNames"
              show-workflow
              entity-label="Pipeline"
              @select="onSelectRun"
            />
          </div>
        </div>
      </template>

      <template #second>
        <div class="runs-detail-shell">
          <MobileBackBar label="Back to pipeline runs" @back="store.selectedRunId = null" />
          <div v-if="!store.detail" class="panel runs-detail-panel">
            <EmptyState
              icon="branch"
              title="Select a pipeline run"
              description="Pick a run on the left to see its member workflow runs and their status."
            />
          </div>
          <div v-else class="panel details runs-detail-panel">
            <div class="runs-section-header">
              <div class="runs-detail-title">
                <h2 class="runs-detail-heading">{{ pipelineName(store.detail.run.pipeline_id) }}</h2>
                <StatusBadge :status="store.detail.run.status" />
              </div>
              <button
                class="btn btn-danger btn-sm"
                :disabled="!isActiveRunStatus(store.detail.run.status)"
                @click="cancelRun(store.detail.run.id)"
              >
                <Icon name="reject" />
                <span>Cancel</span>
              </button>
            </div>
            <dl class="runs-detail-meta">
              <div><dt>Run</dt><dd>#{{ store.detail.run.id }}</dd></div>
              <div><dt>Source</dt><dd>{{ store.detail.run.trigger_source_kind ?? "-" }}</dd></div>
              <div><dt>Started</dt><dd>{{ formatDate(store.detail.run.started_at) }}</dd></div>
              <div><dt>Finished</dt><dd>{{ formatDate(store.detail.run.finished_at) }}</dd></div>
            </dl>
            <p v-if="store.detail.run.message" class="runs-detail-message">
              {{ store.detail.run.message }}
            </p>

            <section class="runs-detail-section">
              <div class="runs-section-header">
                <h2 class="runs-detail-heading">Member Runs</h2>
                <span>{{
                  store.detail.members.length
                    ? `${store.detail.members.length} step run${store.detail.members.length === 1 ? "" : "s"} — click to open`
                    : "No member runs started yet"
                }}</span>
              </div>
              <div v-if="store.detail.members.length" class="table-scroll compact-scroll">
                <RunTable
                  :runs="store.detail.members"
                  :selected-run-id="null"
                  :workflow-names="workflowNames"
                  show-workflow
                  entity-label="Workflow"
                  @select="openMemberRun"
                />
              </div>
              <EmptyState
                v-else
                compact
                icon="runs"
                title="No member runs"
                description="Entry members start when the pipeline run begins; more appear as chained links fire."
              />
            </section>
          </div>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import RunTable from "../components/shared/RunTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { usePipelineRunsStore } from "../../ui/adapters/pinia/pipeline-runs";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { useAppStore } from "../../ui/adapters/pinia/app";
import type { RunSummary } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";
import { countActiveRuns, isActiveRunStatus } from "../../core/utils/status";

const store = usePipelineRunsStore();
const workflows = useWorkflowsStore();
const app = useAppStore();
const selectedPipelineId = ref("");
const starting = ref(false);

// adapt each pipeline run to the shared RunSummary shape so the same RunTable renders both families;
// the pipeline id fills the entity column (labeled "Pipeline") the way workflow_id fills it for runs.
const runRows = computed<RunSummary[]>(() =>
  store.runs.map((run) => ({
    id: run.id,
    workflow_id: run.pipeline_id,
    status: run.status,
    trigger: run.trigger_source_kind ?? undefined,
    created_at: run.created_at,
    started_at: run.started_at,
    finished_at: run.finished_at,
  })),
);

const pipelineNames = computed(() =>
  Object.fromEntries(
    store.pipelines.flatMap((pipeline) => (pipeline.id ? ([[pipeline.id, pipeline.name]] as const) : [])),
  ),
);

const workflowNames = computed(() =>
  Object.fromEntries(
    workflows.workflows.flatMap((workflow) =>
      workflow.id ? ([[workflow.id, workflow.name]] as const) : [],
    ),
  ),
);

const activeRunCount = computed(() => countActiveRuns(store.runs));
const selectedRunLabel = computed(() =>
  store.selectedRunId ? `#${store.selectedRunId}` : "None",
);

function pipelineName(pipelineId: string): string {
  return store.pipelines.find((pipeline) => pipeline.id === pipelineId)?.name ?? pipelineId;
}

function onSelectRun(run: RunSummary): void {
  void store.selectRun(run.id);
}

// click-through: open a pipeline member step in the Runs monitor, loading its workflow run detail.
function openMemberRun(run: RunSummary): void {
  void workflows.selectWorkflowRun(run);
  app.activeTab = "Runs";
}

async function startRun(): Promise<void> {
  if (!selectedPipelineId.value) {
    return;
  }

  starting.value = true;

  try {
    await store.startRun(selectedPipelineId.value);
  } finally {
    starting.value = false;
  }
}

async function cancelRun(pipelineRunId: string): Promise<void> {
  await store.cancelRun(pipelineRunId);
}

onMounted(() => {
  if (!workflows.workflows.length) {
    void workflows.refreshWorkflows();
  }

  void store.refresh();
});
</script>

<style scoped>
.runs-pane {
  overflow: hidden;
}

.runs-list-panel,
.runs-detail-panel {
  min-height: 0;
}

.runs-copy {
  display: grid;
  gap: 4px;
}

.runs-copy p {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.runs-start {
  display: flex;
  gap: 8px;
}

.runs-start select {
  flex: 1;
}

.runs-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.runs-summary div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.runs-summary span,
.runs-section-header span {
  color: var(--text-muted);
  font-size: 12px;
}

.runs-summary strong {
  color: var(--text);
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-hint {
  color: var(--danger, #b42318);
  margin: 0;
  font-size: 12px;
}

.runs-table-scroll {
  flex: 1 1 auto;
}

.runs-table-scroll.is-refreshing {
  opacity: 0.6;
  transition: opacity 120ms ease-out;
}

.runs-detail-shell {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
}

.runs-detail-panel {
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.runs-detail-title {
  display: flex;
  align-items: center;
  gap: 10px;
}

.runs-detail-heading {
  margin: 0;
}

.runs-detail-meta {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 8px;
  margin: 0;
}

.runs-detail-meta div {
  display: grid;
  gap: 2px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 8px 10px;
}

.runs-detail-meta dt {
  color: var(--text-muted);
  font-size: 12px;
}

.runs-detail-meta dd {
  margin: 0;
  color: var(--text);
  font-size: 13px;
}

.runs-detail-message {
  margin: 0;
  color: var(--text-muted);
  font-size: 13px;
}

.runs-detail-section {
  display: grid;
  gap: 8px;
  border-top: 1px solid var(--border-subtle);
  padding-top: 12px;
}

.runs-section-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

@media (max-width: 980px) {
  .runs-summary {
    grid-template-columns: 1fr;
  }
}
</style>
