<template>
  <section class="pane runs-pane">
    <SplitPane class="split" storage-key="command-center.runs.split" :initial-first-pct="42" :min-first="340" :min-second="380">
      <template #first>
      <div class="panel">
        <h2>Runs</h2>
        <RunTable :runs="tasks.recentRuns" :selected-run-id="tasks.selectedRunId" @select="tasks.selectRun" />
      </div>
      </template>
      <template #second>
      <div class="panel details">
        <h2>Structured Result</h2>
        <pre class="output">{{ selectedOutput }}</pre>
        <h2>Run Output Chunks</h2>
        <pre class="output">{{ logOutput }}</pre>
        <h2>Artifacts</h2>
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
              <tr v-for="artifact in tasks.artifacts" :key="artifact.id">
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
  </section>
</template>

<script setup lang="ts">
import RunTable from "../components/shared/RunTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useTasksStore } from "../stores/tasks";
import { useRunLogStream } from "../composables/useRunLogStream";
import { formatDate, pretty } from "../utils/format";
import { computed, ref, watch } from "vue";

const tasks = useTasksStore();
const selectedOutput = computed(() => pretty(tasks.selectedRun?.output_json ?? {}));

const selectedRunIdRef = ref(tasks.selectedRunId);
watch(() => tasks.selectedRunId, (id) => { selectedRunIdRef.value = id; });
const { chunks: logChunks } = useRunLogStream(selectedRunIdRef);
const logOutput = computed(() => {
  if (logChunks.value.length > 0) return logChunks.value.map(c => `[${c.stream}] ${c.content}`).join("\n");
  return tasks.runOutput;
});
</script>
