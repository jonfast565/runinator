<template>
  <section class="pane runs-pane">
    <SplitPane class="runs-layout" storage-key="command-center.runs.split" :initial-first-pct="28" :min-first="340" :min-second="720">
      <template #first>
        <div class="panel runs-list-panel">
          <h2>Runs</h2>
          <RunTable
            :runs="workflows.recentWorkflowRuns"
            :selected-run-id="workflows.selectedWorkflowRunId"
            :workflow-names="workflowNames"
            show-workflow
            @select="workflows.selectWorkflowRun"
          />
        </div>
      </template>
      <template #second>
        <div class="runs-detail-shell">
          <RunTabsBar />
          <SplitPane
          class="runs-detail-split"
          orientation="vertical"
          storage-key="command-center.runs.detail-vertical-split"
          :initial-first-pct="55"
          :min-first="260"
          :min-second="320"
        >
          <template #first>
            <WorkflowRunGraph />
          </template>
          <template #second>
            <div class="panel details runs-detail-panel">
              <WorkflowRunDetail />
              <h2 class="runs-detail-heading">Structured Result</h2>
              <pre class="output runs-detail-output">{{ selectedOutput }}</pre>
              <h2 class="runs-detail-heading">Run Output Chunks</h2>
              <LogPanel :chunks="logChunks" :last-chunk-at="lastLogChunkAt" :fallback-text="workflows.workflowRunDetailText" />
              <h2 class="runs-detail-heading">Selected Node Artifacts</h2>
              <div class="table-scroll compact-scroll">
                <table>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>MIME</th>
                      <th>Size</th>
                      <th>URI</th>
                      <th>Created</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="artifact in artifacts" :key="artifact.id">
                      <td>{{ artifact.name }}</td>
                      <td>{{ artifact.mime_type }}</td>
                      <td>{{ artifact.size_bytes }}</td>
                      <td>{{ artifact.uri }}</td>
                      <td>{{ formatDate(artifact.created_at) }}</td>
                    </tr>
                  </tbody>
                </table>
              </div>
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
import { fetchWorkflowNodeRunArtifacts } from "../api/commandCenterApi";
import RunTable from "../components/shared/RunTable.vue";
import RunTabsBar from "../components/shared/RunTabsBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import LogPanel from "../components/workflow/LogPanel.vue";
import WorkflowRunDetail from "../components/workflow/WorkflowRunDetail.vue";
import WorkflowRunGraph from "../components/workflow/WorkflowRunGraph.vue";
import { useWorkflowRunStream } from "../composables/useWorkflowRunStream";
import { useWorkflowNodeRunLogStream } from "../composables/useWorkflowNodeRunLogStream";
import { useAppStore } from "../stores/app";
import { useWorkflowsStore } from "../stores/workflows";
import type { RunArtifact } from "../types/models";
import { formatDate, pretty } from "../utils/format";

const app = useAppStore();
const workflows = useWorkflowsStore();
const artifacts = ref<RunArtifact[]>([]);
const selectedOutput = computed(() => pretty(workflows.workflowRunDetail?.run.output_json ?? {}));
const selectedNodeRunIdRef = ref(workflows.selectedWorkflowNodeRunId);
const workflowNames = computed(() => Object.fromEntries(workflows.workflows.filter((workflow) => workflow.id).map((workflow) => [workflow.id!, workflow.name])));

useWorkflowRunStream();

watch(() => workflows.selectedWorkflowNodeRunId, (id) => { selectedNodeRunIdRef.value = id; }, { immediate: true });
watch(() => workflows.selectedWorkflowNodeRunId, async (id) => {
  artifacts.value = id > 0 ? await app.runOperation("Loading node artifacts", () => fetchWorkflowNodeRunArtifacts(id)).catch(() => []) : [];
}, { immediate: true });

const { chunks: logChunks, lastChunkAt: lastLogChunkAt } = useWorkflowNodeRunLogStream(selectedNodeRunIdRef);
</script>

<style scoped>
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
</style>
