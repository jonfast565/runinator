<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.pipeline-runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="!!store.selectedRunId"
    >
      <template #first>
        <div class="panel min-h-0">
          <PanelHeader
            title="Pipeline Runs"
            description="First-class pipeline executions and the member workflow runs they orchestrate."
          >
            <button class="btn" :disabled="store.loading" @click="store.refresh">
              <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing pipeline runs" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </PanelHeader>
          <div class="flex gap-2">
            <select v-model="selectedPipelineId" class="input flex-1">
              <option value="">Start a pipeline…</option>
              <option
                v-for="pipeline in store.pipelines"
                :key="pipeline.id ?? ''"
                :value="pipeline.id ?? ''"
              >
                {{ pipeline.name }}
              </option>
            </select>
            <button
              class="btn btn-primary"
              :disabled="!selectedPipelineId || starting"
              @click="startRun"
            >
              <Icon name="runs" />
              <span>Start run</span>
            </button>
          </div>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard label="Visible" :value="store.runs.length" />
            <MetricCard label="Active" :value="activeRunCount" />
            <MetricCard label="Selected" :value="selectedRunLabel" />
          </div>
          <p v-if="store.error" class="error m-0 text-xs">{{ store.error }}</p>
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
          <div
            v-else
            class="table-scroll min-h-0 flex-1"
            :class="{ 'opacity-60 transition-opacity duration-100': store.loading }"
          >
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
        <div class="flex min-h-0 flex-1 flex-col">
          <MobileBackBar label="Back to pipeline runs" @back="store.selectedRunId = null" />
          <div v-if="!store.detail" class="panel min-h-0">
            <EmptyState
              icon="branch"
              title="Select a pipeline run"
              description="Pick a run on the left to see its member workflow runs and their status."
            />
          </div>
          <div v-else class="panel details flex min-h-0 flex-col gap-3 overflow-auto">
            <div class="flex items-baseline justify-between gap-2">
              <div class="flex items-center gap-2.5">
                <h2 class="m-0 text-base font-semibold text-fg">
                  {{ pipelineName(store.detail.run.pipeline_id) }}
                </h2>
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
            <dl class="m-0 grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-2">
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Run</dt>
                <dd class="m-0 text-[13px] text-fg">#{{ store.detail.run.id }}</dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Source</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ store.detail.run.trigger_source_kind ?? "-" }}
                </dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Started</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ formatDate(store.detail.run.started_at) }}
                </dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Finished</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ formatDate(store.detail.run.finished_at) }}
                </dd>
              </div>
            </dl>
            <p v-if="store.detail.run.message" class="m-0 text-[13px] text-fg-muted">
              {{ store.detail.run.message }}
            </p>

            <section class="grid gap-2 border-t border-border-subtle pt-3">
              <div class="flex items-baseline justify-between gap-2">
                <h2 class="m-0 text-base font-semibold text-fg">Member Runs</h2>
                <span class="text-xs text-fg-muted">{{
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
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
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
