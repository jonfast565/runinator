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
        <pre class="output">{{ tasks.runOutput }}</pre>
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
import { formatDate, pretty } from "../utils/format";
import { computed } from "vue";

const tasks = useTasksStore();
const selectedOutput = computed(() => pretty(tasks.selectedRun?.output_json ?? {}));
</script>
