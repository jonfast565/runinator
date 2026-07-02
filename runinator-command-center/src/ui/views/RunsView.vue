<template>
  <section class="pane runs-pane">
    <SplitPane
      class="runs-layout"
      storage-key="command-center.runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      mobile-mode="toggle"
      :mobile-detail-active="!!workflows.selectedWorkflowRunId"
    >
      <template #first>
        <div class="panel runs-list-panel">
          <div class="panel-toolbar">
            <div class="runs-copy">
              <h2>Runs</h2>
              <p>Recent workflow executions, filtered by the current search when present.</p>
            </div>
            <button class="btn" @click="workflows.fetchRecentWorkflowRuns()">
              <span>Refresh</span>
            </button>
          </div>
          <div class="runs-summary">
            <div>
              <span>Visible</span>
              <strong>{{ workflows.recentWorkflowRuns.length }}</strong>
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
          <EmptyState
            v-if="!workflows.recentWorkflowRuns.length"
            compact
            :icon="app.searchQuery ? 'search' : 'runs'"
            :title="app.searchQuery ? 'No matches' : 'No runs yet'"
            :description="
              app.searchQuery
                ? `No runs match “${app.searchQuery}”.`
                : 'Runs appear here once a workflow is executed. Run one from the Workflows tab.'
            "
          />
          <div v-else class="table-scroll runs-table-scroll">
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
        <div class="runs-detail-shell">
          <MobileBackBar label="Back to runs" @back="workflows.selectedWorkflowRunId = null" />
          <RunTabsBar />
          <SplitPane
            class="runs-detail-split"
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
              <div class="panel details runs-detail-panel">
                <WorkflowRunDetail />
                <section class="runs-detail-section">
                  <div class="runs-section-header">
                    <h2 class="runs-detail-heading">Structured Result</h2>
                    <span>Workflow output JSON</span>
                  </div>
                  <JsonEditor
                    class="runs-detail-output"
                    :model-value="selectedOutput"
                    readonly
                    title=""
                  />
                </section>
                <section class="runs-detail-section">
                  <div class="runs-section-header">
                    <h2 class="runs-detail-heading">Run Output Chunks</h2>
                    <span>Streamed log and output segments</span>
                  </div>
                  <LogPanel
                    :chunks="logChunks"
                    :last-chunk-at="lastLogChunkAt"
                    :fallback-text="workflows.workflowRunDetailText"
                  />
                </section>
                <section class="runs-detail-section">
                  <div class="runs-section-header">
                    <h2 class="runs-detail-heading">Selected Node Artifacts</h2>
                    <span>{{
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
                <section class="runs-detail-section">
                  <div class="runs-section-header">
                    <h2 class="runs-detail-heading">Artifacts</h2>
                    <span>{{
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
import JsonEditor from "../components/shared/JsonEditor.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import RunTable from "../components/shared/RunTable.vue";
import RunTabsBar from "../components/shared/RunTabsBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import LogPanel from "../components/workflow/LogPanel.vue";
import WorkflowRunDetail from "../components/workflow/WorkflowRunDetail.vue";
import WorkflowRunGraph from "../components/workflow/WorkflowRunGraph.vue";
import { useWorkflowRunStream } from "../composables/useWorkflowRunStream";
import { useWorkflowNodeRunLogStream } from "../composables/useWorkflowNodeRunLogStream";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import type { RunArtifact, WorkflowRunArtifact } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";

const app = useAppStore();
const workflows = useWorkflowsStore();
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
const TERMINAL_STATUSES = new Set(["succeeded", "failed", "canceled", "timed_out"]);
const activeRunCount = computed(
  () => workflows.recentWorkflowRuns.filter((run) => !TERMINAL_STATUSES.has(run.status)).length,
);
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
}

.runs-table-scroll {
  flex: 1 1 auto;
}

.runs-detail-shell {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
}

.runs-detail-shell > .split-pane {
  flex: 1 1 auto;
  min-height: 0;
}

.runs-detail-panel {
  overflow: auto;
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

.runs-detail-heading {
  margin: 0;
}

.runs-detail-output {
  flex: 0 0 auto;
  min-height: 0;
}

.runs-detail-output :deep(.json-editor-container) {
  max-height: 260px;
}

@media (max-width: 980px) {
  .runs-summary {
    grid-template-columns: 1fr;
  }
}
</style>
