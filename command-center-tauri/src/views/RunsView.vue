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
        <SplitPane class="runs-detail-split" storage-key="command-center.runs.detail-split" :initial-first-pct="58" :min-first="420" :min-second="360">
          <template #first>
            <WorkflowRunGraph />
          </template>
          <template #second>
            <div class="panel details runs-detail-panel">
              <WorkflowRunDetail />
              <h2>Structured Result</h2>
              <pre class="output">{{ selectedOutput }}</pre>
              <h2>Run Output Chunks</h2>
              <pre class="output">{{ logOutput }}</pre>
              <h2>Selected Node Artifacts</h2>
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
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { fetchWorkflowNodeRunArtifacts } from "../api/commandCenterApi";
import RunTable from "../components/shared/RunTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
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

const { chunks: logChunks } = useWorkflowNodeRunLogStream(selectedNodeRunIdRef);
const logOutput = computed(() => {
  if (logChunks.value.length > 0) return logChunks.value.map(c => `[${c.stream}] ${c.content}`).join("\n");
  return workflows.workflowRunDetailText;
});
</script>
