<template>
  <section class="pane runs-pane">
    <div class="split">
      <div class="panel">
        <h2>Runs</h2>
        <RunTable :runs="tasks.runs" :selected-run-id="tasks.selectedRunId" @select="tasks.selectRun" />
      </div>
      <div class="panel details">
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
    </div>
  </section>
</template>

<script setup lang="ts">
import RunTable from "../components/shared/RunTable.vue";
import { useTasksStore } from "../stores/tasks";
import { formatDate } from "../utils/format";

const tasks = useTasksStore();
</script>
