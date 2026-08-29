<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      first-label="Runs"
      first-icon="runs"
      mobile-mode="toggle"
      :mobile-detail-active="!!workflows.selectedWorkflowRunId"
    >
      <template #first>
        <div class="panel min-h-0">
          <PanelHeader
            title="Runs"
            description="Recent workflow executions, filtered by the current search when present."
          >
            <button
              class="btn"
              :disabled="loadingRuns"
              @click="workflows.fetchRecentWorkflowRuns()"
            >
              <LoadingSpinner v-if="loadingRuns" size="sm" label="Refreshing runs" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </PanelHeader>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard label="Visible" :value="workflows.recentWorkflowRuns.length" />
            <MetricCard label="Active" :value="activeRunCount" />
            <MetricCard label="Selected" :value="selectedRunLabel" />
          </div>
          <EmptyState
            v-if="loadingRuns && !workflows.recentWorkflowRuns.length"
            compact
            loading
            title="Loading runs"
            :loading-message="loadingRunsMessage"
          />
          <EmptyState
            v-else-if="!workflows.recentWorkflowRuns.length"
            compact
            :icon="app.searchQuery ? 'search' : 'runs'"
            :title="app.searchQuery ? 'No matches' : 'No runs yet'"
            :description="
              app.searchQuery
                ? `No runs match “${app.searchQuery}”.`
                : 'Runs appear here once a workflow is executed. Run one from the Workflows tab.'
            "
          />
          <template v-else>
            <BulkActionBar
              class="mb-2"
              noun="run"
              :count="selection.count.value"
              :actions="bulkActions"
              :busy="bulkBusy"
              @run="runBulkAction"
              @clear="selection.clear"
            />
            <div
              class="table-scroll min-h-0 flex-1"
              :class="{ 'opacity-60 transition-opacity duration-100': loadingRuns }"
            >
              <RunTable
                :runs="workflows.recentWorkflowRuns"
                :selected-run-id="workflows.selectedWorkflowRunId"
                :workflow-names="workflowNames"
                show-workflow
                selectable
                :selected-run-ids="selection.selectedKeys.value as string[]"
                :all-selected="selection.allSelected.value"
                :some-selected="selection.someSelected.value"
                deletable
                @select="workflows.selectWorkflowRun"
                @toggle-row="selection.toggle"
                @toggle-all="selection.toggleAll"
                @delete="deleteRun"
              />
            </div>
          </template>
        </div>
      </template>
      <template #second>
        <div class="flex min-h-0 flex-1 flex-col [&_.split-pane]:min-h-0 [&_.split-pane]:flex-1">
          <MobileBackBar label="Back to runs" @back="workflows.selectedWorkflowRunId = null" />
          <RunTabsBar />
          <SplitPane
            class="min-h-0 flex-1"
            orientation="vertical"
            storage-key="command-center.runs.detail-vertical-split"
            :initial-first-pct="55"
            :min-first="260"
            :min-second="320"
            collapsible-second
            second-label="Details"
            second-icon="info"
          >
            <template #first>
              <WorkflowRunGraph />
            </template>
            <template #second>
              <div ref="runDetailScroller" class="panel details run-detail-scroll min-h-0">
                <WorkflowRunDetail />
                <IngressTimeline />
                <section class="grid gap-2 border-t border-border-subtle pt-3">
                  <div class="flex items-baseline justify-between gap-2">
                    <h2 class="m-0 text-base font-semibold text-fg">Structured Result</h2>
                    <span class="text-xs text-fg-muted">Workflow output JSON</span>
                  </div>
                  <JsonEditor
                    class="min-h-0 shrink-0 [&_.json-editor-container]:max-h-[260px]"
                    :model-value="selectedOutput"
                    readonly
                    title=""
                  />
                </section>
                <section class="grid gap-2 border-t border-border-subtle pt-3">
                  <div class="flex items-baseline justify-between gap-2">
                    <h2 class="m-0 text-base font-semibold text-fg">Step Output</h2>
                    <span class="text-xs text-fg-muted">Selected step diagnostics and logs</span>
                  </div>
                  <LogPanel
                    :chunks="logChunks"
                    :last-chunk-at="lastLogChunkAt"
                    :context="selectedLogContext"
                  />
                </section>
                <section class="grid gap-2 border-t border-border-subtle pt-3">
                  <div class="flex items-baseline justify-between gap-2">
                    <h2 class="m-0 text-base font-semibold text-fg">Selected Node Artifacts</h2>
                    <span class="text-xs text-fg-muted">{{
                      artifacts.length
                        ? `${artifacts.length} attached`
                        : "No artifacts on the selected node"
                    }}</span>
                  </div>
                  <div class="table-scroll compact-scroll">
                    <DataTable bare>
                      <thead>
                        <tr>
                          <th>Name</th>
                          <th>MIME</th>
                          <th>Size</th>
                          <th>URI</th>
                          <th>Created</th>
                          <th></th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr v-if="!artifacts.length" class="muted">
                          <td colspan="6">No artifacts available.</td>
                        </tr>
                        <tr v-for="artifact in artifacts" :key="artifact.id">
                          <td>{{ artifact.name }}</td>
                          <td>{{ artifact.mime_type }}</td>
                          <td>{{ artifact.size_bytes }}</td>
                          <td>{{ artifact.uri }}</td>
                          <td>{{ formatDate(artifact.created_at) }}</td>
                          <td>
                            <button
                              class="btn btn-sm"
                              type="button"
                              @click="
                                downloadArtifact(
                                  artifact.workflow_node_run_id ??
                                    workflows.selectedWorkflowNodeRunId ??
                                    '',
                                  artifact.id,
                                  artifact.name,
                                )
                              "
                            >
                              Download
                            </button>
                          </td>
                        </tr>
                      </tbody>
                    </DataTable>
                  </div>
                </section>
                <section class="grid gap-2 border-t border-border-subtle pt-3">
                  <div class="flex items-baseline justify-between gap-2">
                    <h2 class="m-0 text-base font-semibold text-fg">Artifacts</h2>
                    <span class="text-xs text-fg-muted">{{
                      runArtifacts.length
                        ? `${runArtifacts.length} for this run`
                        : "No artifacts for this run"
                    }}</span>
                  </div>
                  <div class="table-scroll compact-scroll">
                    <DataTable bare>
                      <thead>
                        <tr>
                          <th>Name</th>
                          <th>From node</th>
                          <th>MIME</th>
                          <th>Size</th>
                          <th>Created</th>
                          <th></th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr v-if="!runArtifacts.length" class="muted">
                          <td colspan="6">No artifacts available.</td>
                        </tr>
                        <tr v-for="artifact in runArtifacts" :key="artifact.id">
                          <td>{{ artifact.name }}</td>
                          <td>{{ artifact.node_id }}</td>
                          <td>{{ artifact.mime_type }}</td>
                          <td>{{ artifact.size_bytes }}</td>
                          <td>{{ formatDate(artifact.created_at) }}</td>
                          <td>
                            <button
                              class="btn btn-sm"
                              type="button"
                              @click="
                                downloadArtifact(artifact.node_id, artifact.id, artifact.name)
                              "
                            >
                              Download
                            </button>
                          </td>
                        </tr>
                      </tbody>
                    </DataTable>
                  </div>
                </section>
              </div>
            </template>
          </SplitPane>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { workflowRunExtrasService } from "../../core/services";
import { downloadWorkflowEffectArtifact } from "../../core/api/commandCenterApi";
import { downloadBlob } from "../adapters/browser/files";
import BulkActionBar, { type BulkAction } from "../components/shared/BulkActionBar.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import IngressTimeline from "../components/shared/IngressTimeline.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import RunTable from "../components/shared/RunTable.vue";
import RunTabsBar from "../components/shared/RunTabsBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import LogPanel from "../components/workflow/LogPanel.vue";
import WorkflowRunDetail from "../components/workflow/WorkflowRunDetail.vue";
import WorkflowRunGraph from "../components/workflow/WorkflowRunGraph.vue";
import { useBulkSelection } from "../composables/useBulkSelection";
import { useWorkflowRunStream } from "../composables/useWorkflowRunStream";
import { useOperationLoading } from "../composables/useOperationLoading";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import {
  workflowEffectId,
  type RunArtifact,
  type RunChunk,
  type WorkflowRunArtifact,
} from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";
import { countActiveRuns, isActiveRunStatus } from "../../core/utils/status";

const app = useAppStore();
const workflows = useWorkflowsStore();
const { isLoading: loadingRuns, loadingMessage: loadingRunsMessage } =
  useOperationLoading("Loading workflow runs");
const artifacts = ref<RunArtifact[]>([]);
const runArtifacts = ref<WorkflowRunArtifact[]>([]);
const logChunks = ref<RunChunk[]>([]);
const lastLogChunkAt = ref(0);
const runDetailScroller = ref<HTMLElement | null>(null);

const selectedOutput = computed(() => pretty(workflows.workflowRunDetail?.run.output_json ?? {}));
const workflowNames = computed(() =>
  Object.fromEntries(
    workflows.workflows.flatMap((workflow) =>
      workflow.id ? ([[workflow.id, workflow.name]] as const) : [],
    ),
  ),
);
const activeRunCount = computed(() => countActiveRuns(workflows.recentWorkflowRuns));
const selectedRunLabel = computed(() =>
  workflows.selectedWorkflowRunId ? `#${workflows.selectedWorkflowRunId}` : "None",
);
const selectedLogContext = computed(() => {
  const detail = workflows.workflowRunDetail;

  if (!detail) {
    return null;
  }

  const selectedNodeId = workflows.selectedWorkflowRunNodeId;
  const selectedEffectId = workflows.selectedWorkflowNodeRunId;
  const node = [...detail.nodes]
    .reverse()
    .find(
      (candidate) =>
        workflowEffectId(candidate) === selectedEffectId || candidate.node_id === selectedNodeId,
    );
  const effect =
    detail.effects?.find((candidate) => candidate.id === selectedEffectId) ??
    detail.effects?.find((candidate) => candidate.node_id === selectedNodeId) ??
    null;
  const request =
    effect?.request && typeof effect.request === "object" && !Array.isArray(effect.request)
      ? (effect.request as Record<string, unknown>)
      : null;
  const provider = typeof request?.provider === "string" ? request.provider : "";
  const functionName = typeof request?.function === "string" ? request.function : "";

  return {
    runId: detail.run.id,
    runStatus: detail.run.status,
    nodeId: node?.node_id ?? selectedNodeId,
    nodeStatus: node?.status ?? null,
    effectId: effect?.id ?? selectedEffectId,
    effectStatus: effect?.status ?? null,
    continuationId: effect?.continuation_id ?? node?.cursor_id ?? null,
    attempt: effect?.attempt ?? node?.attempt ?? null,
    action: provider && functionName ? `${provider}.${functionName}` : null,
    message: effect?.message ?? node?.message ?? detail.run.message ?? null,
  };
});

async function deleteRun(run: (typeof recentRuns.value)[number]): Promise<void> {
  if (!window.confirm("Permanently delete this workflow run and all execution history?")) {
    return;
  }

  await workflows.deleteWorkflowRunById(run.id);
  selection.clear();
}

const recentRuns = computed(() => workflows.recentWorkflowRuns);
const selection = useBulkSelection(recentRuns, (run) => run.id);
const bulkBusy = ref("");

async function downloadArtifact(effectId: string, eventId: string, name: string) {
  if (!effectId || !eventId) {
    return;
  }

  const blob = await downloadWorkflowEffectArtifact(effectId, eventId);
  downloadBlob(name || "artifact", blob);
}

const bulkActions = computed<BulkAction[]>(() => [
  {
    key: "cancel",
    label: "Cancel",
    icon: "stop",
    variant: "danger",
    // cancelling a settled run is a guaranteed failure, so offer it only when one is still active.
    disabled: !selection.selectedRows.value.some((run) => isActiveRunStatus(run.status)),
  },
  { key: "replay", label: "Replay", icon: "replay" },
  { key: "delete", label: "Delete", icon: "trash", variant: "danger" },
]);

async function runBulkAction(key: string) {
  const selected = selection.selectedRows.value;

  if (!selected.length || bulkBusy.value) {
    return;
  }

  if (
    key === "replay" &&
    !window.confirm(
      `Replay ${String(selected.length)} run${selected.length === 1 ? "" : "s"}?\n\nEach replay starts a new run from the beginning.`,
    )
  ) {
    return;
  }

  bulkBusy.value = key;
  let completed = false;

  try {
    if (key === "cancel") {
      await workflows.cancelWorkflowRuns(selected);
      completed = true;
    } else if (key === "replay") {
      await workflows.replayWorkflowRuns(selected);
      completed = true;
    } else if (key === "delete") {
      completed = await workflows.deleteWorkflowRuns(selected);
    }
  } finally {
    bulkBusy.value = "";
  }

  if (completed) {
    selection.clear();
  }
}

useWorkflowRunStream();

watch(
  () => [workflows.selectedWorkflowNodeRunId, workflows.workflowRunDetail] as const,
  async () => {
    const id = workflows.selectedWorkflowNodeRunId;
    const [nextArtifacts, nextChunks] = id
      ? await Promise.all([
          workflowRunExtrasService.fetchNodeRunArtifacts(id),
          workflowRunExtrasService.fetchNodeRunChunks(id),
        ])
      : [[], []];

    // A rapid step selection can leave an older output request in flight. Do not let its response
    // replace the logs/artifacts for the step that is currently selected.
    if (id !== workflows.selectedWorkflowNodeRunId) {
      return;
    }

    artifacts.value = nextArtifacts;
    logChunks.value = nextChunks;
    const latestChunkAt = Math.max(
      ...nextChunks.map((chunk) => Date.parse(chunk.created_at)).filter(Number.isFinite),
    );
    lastLogChunkAt.value = Number.isFinite(latestChunkAt) ? latestChunkAt : 0;
  },
  { immediate: true },
);
watch(
  () => workflows.selectedWorkflowRunId,
  async (id) => {
    await nextTick();
    const scroller = runDetailScroller.value;

    if (scroller) {
      scroller.scrollTop = 0;
      const stackedPane = scroller.closest<HTMLElement>(".split-pane-stacked");

      if (stackedPane) {
        stackedPane.scrollTop = 0;
      }
    }

    runArtifacts.value = id ? await workflowRunExtrasService.fetchRunArtifacts(id) : [];
  },
  { immediate: true },
);
</script>
