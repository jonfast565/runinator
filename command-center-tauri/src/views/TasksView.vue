<template>
  <section class="pane tasks-pane">
    <SplitPane class="tasks-dashboard" storage-key="command-center.tasks.split" :initial-first-pct="56" :min-first="420" :min-second="360">
      <template #first>
      <div class="panel task-list-panel">
        <div class="panel-toolbar">
          <h2>Tasks</h2>
          <div>
            <button @click="tasks.openNewTask">Add</button>
            <button :disabled="!tasks.selectedTask" @click="tasks.openSelectedTask">Edit</button>
          </div>
        </div>
        <DataTable>
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Cron</th>
                <th>Next Run</th>
                <th>Enabled</th>
                <th>Timeout</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="task in tasks.filteredTasks"
                :key="task.id ?? task.name"
                :class="{ selected: tasks.selectedTaskId === task.id, muted: !task.enabled }"
                @click="tasks.selectTask(task)"
                @dblclick="tasks.openTask(task)"
              >
                <td>{{ task.name }}</td>
                <td>{{ task.cron_schedule }}</td>
                <td>{{ formatDate(task.next_execution) }}</td>
                <td><StatusBadge :status="task.enabled" /></td>
                <td>{{ task.timeout }} ms</td>
                <td>{{ task.action_name }}</td>
              </tr>
            </tbody>
          </table>
        </DataTable>
      </div>
      </template>

      <template #second>
      <div class="panel selected-detail-panel">
        <div class="detail-header">
          <div>
            <h2>{{ tasks.selectedTask?.name ?? "No task selected" }}</h2>
            <span>{{ tasks.selectedTask?.action_name ?? "" }} {{ tasks.selectedTask?.action_function ?? "" }}</span>
          </div>
          <StatusBadge v-if="tasks.selectedTask" :status="tasks.selectedTask.enabled" />
        </div>
        <div class="metrics-row">
          <div><span>Cron</span><strong>{{ tasks.selectedTask?.cron_schedule ?? "-" }}</strong></div>
          <div><span>Next</span><strong>{{ formatDate(tasks.selectedTask?.next_execution) }}</strong></div>
          <div><span>Runs</span><strong>{{ tasks.recentRuns.length }}</strong></div>
        </div>
        <RunTable :runs="tasks.recentRuns" :selected-run-id="tasks.selectedRunId" @select="tasks.selectRun" />
      </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { useTasksStore } from "../stores/tasks";
import { formatDate } from "../utils/format";
import DataTable from "../components/shared/DataTable.vue";
import RunTable from "../components/shared/RunTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";

const tasks = useTasksStore();
</script>
