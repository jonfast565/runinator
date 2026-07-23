<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="!!workflows.selectedWorkflowRunId"
    >
      <template #first>
        <div class="panel min-h-0">
          <div class="panel-toolbar">
            <div class="grid gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">Runs</h2>
              <p class="m-0 text-xs text-fg-muted">
                Recent workflow executions, filtered by the current search when present.
              </p>
            </div>
            <button class="btn" :disabled="loadingRuns" @click="workflows.fetchRecentWorkflowRuns()">
              <LoadingSpinner v-if="loadingRuns" size="sm" label="Refreshing runs" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </div>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <div
              class="grid gap-1 rounded-md border border-border-subtle bg-surface-subtle px-3 py-2.5"
            >
              <span class="text-xs text-fg-muted">Visible</span>
              <strong class="text-sm text-fg">{{ workflows.recentWorkflowRuns.length }}</strong>
            </div>
            <div
              class="grid gap-1 rounded-md border border-border-subtle bg-surface-subtle px-3 py-2.5"
            >
              <span class="text-xs text-fg-muted">Active</span>
              <strong class="text-sm text-fg">{{ activeRunCount }}</strong>
            </div>
            <div
              class="grid gap-1 rounded-md border border-border-subtle bg-surface-subtle px-3 py-2.5"
            >
              <span class="text-xs text-fg-muted">Selected</span>
              <strong class="truncate text-sm text-fg">{{ selectedRunLabel }}</strong>
            </div>
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
          <div
            v-else
            class="table-scroll min-h-0 flex-1"
            :class="{ 'opacity-60 transition-opacity duration-100': loadingRuns }"
          >
            <RunTable
              :runs="workflows.recentWorkflowRuns"
              :selected-run-id="workflows.selectedWorkflowRunId"
              :workflow-names="workflowNames"
              show-workflow
              @select="workflows.selectWorkflowRun"
            />
          </div>
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
          >
            <template #first>
              <WorkflowRunGraph />
            </template>
            <template #second>
              <div class="panel details min-h-0 overflow-auto">
                <WorkflowRunDetail />
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
                    <h2 class="m-0 text-base font-semibold text-fg">Run Output Chunks</h2>
                    <span class="text-xs text-fg-muted">Streamed log and output segments</span>
                  </div>
                  <LogPanel
                    :chunks="logChunks"
                    :last-chunk-at="lastLogChunkAt"
                    :fallback-text="workflows.workflowRunDetailText"
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
                    <table>
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
                            <button class="btn" @click="download(artifact.id, artifact.name)">
                              Download
                            </button>
                          </td>
                        </tr>
                      </tbody>
                    </table>
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
                    <table>
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
                              class="btn"
                              @click="download(artifact.artifact_id, artifact.name)"
                            >
                              Download
                            </button>
                          </td>
                        </tr>
                      </tbody>
                    </table>
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
import { computed, ref, watch } from "vue";
import { workflowRunExtrasService } from "../../core/services";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import RunTable from "../components/shared/RunTable.vue";
import RunTabsBar from "../components/shared/RunTabsBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import LogPanel from "../components/workflow/LogPanel.vue";
import WorkflowRunDetail from "../components/workflow/WorkflowRunDetail.vue";
import WorkflowRunGraph from "../components/workflow/WorkflowRunGraph.vue";
import { useWorkflowRunStream } from "../composables/useWorkflowRunStream";
import { useWorkflowNodeRunLogStream } from "../composables/useWorkflowNodeRunLogStream";
import { useOperationLoading } from "../composables/useOperationLoading";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import type { RunArtifact, WorkflowRunArtifact } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";
import { countActiveRuns } from "../../core/utils/status";

const app = useAppStore();
const workflows = useWorkflowsStore();
const { isLoading: loadingRuns, loadingMessage: loadingRunsMessage } =
  useOperationLoading("Loading workflow runs");
const artifacts = ref<RunArtifact[]>([]);
const runArtifacts = ref<WorkflowRunArtifact[]>([]);

async function download(artifactId: string, name: string) {
  await workflowRunExtrasService.downloadArtifact(artifactId, name).catch((error: unknown) => {
    app.setError(String(error));
  });
}

const selectedOutput = computed(() => pretty(workflows.workflowRunDetail?.run.output_json ?? {}));
const selectedNodeRunIdRef = ref(workflows.selectedWorkflowNodeRunId);
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

useWorkflowRunStream();

watch(
  () => workflows.selectedWorkflowNodeRunId,
  (id) => {
    selectedNodeRunIdRef.value = id;
  },
  { immediate: true },
);
watch(
  () => workflows.selectedWorkflowNodeRunId,
  async (id) => {
    artifacts.value = id ? await workflowRunExtrasService.fetchNodeRunArtifacts(id) : [];
  },
  { immediate: true },
);
watch(
  () => workflows.selectedWorkflowRunId,
  async (id) => {
    runArtifacts.value = id ? await workflowRunExtrasService.fetchRunArtifacts(id) : [];
  },
  { immediate: true },
);

const { chunks: logChunks, lastChunkAt: lastLogChunkAt } =
  useWorkflowNodeRunLogStream(selectedNodeRunIdRef);
</script>
