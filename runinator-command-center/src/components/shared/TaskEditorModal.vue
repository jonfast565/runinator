<template>
  <div class="modal-backdrop">
    <form class="modal task-modal" @submit.prevent="tasks.submitTaskEditor">
      <header class="modal-header">
        <div>
          <h2>{{ tasks.taskEditorCreating ? "Add Task" : "Edit Task" }}</h2>
          <span>{{ tasks.taskDraft.name || "Task" }}</span>
        </div>
        <button type="button" @click="tasks.closeTaskEditor">Close</button>
      </header>

      <section class="form-section">
        <h3>Task</h3>
        <div class="form-grid">
          <label>Name <input v-model="tasks.taskDraft.name" /></label>
          <label>Cron <input v-model="tasks.taskDraft.cron_schedule" /></label>
          <label>Action Name <input v-model="tasks.taskDraft.action_name" /></label>
          <label>Action Function <input v-model="tasks.taskDraft.action_function" /></label>
          <label>Timeout <input v-model.number="tasks.taskDraft.timeout" type="number" min="1" /></label>
          <label class="checkbox"><input v-model="tasks.taskDraft.enabled" type="checkbox" /> Enabled</label>
        </div>
      </section>

      <section class="form-section">
        <h3>JSON</h3>
        <div class="form-field">
          <span class="form-field-label">Configuration</span>
          <JsonEditor v-model="tasks.taskJson.configuration" />
        </div>
      </section>

      <p v-if="tasks.taskEditorError" class="form-error">{{ tasks.taskEditorError }}</p>
      <div class="modal-actions">
        <button type="button" @click="tasks.closeTaskEditor">Cancel</button>
        <button type="submit">Save</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { useTasksStore } from "../../stores/tasks";
import JsonEditor from "./JsonEditor.vue";

const tasks = useTasksStore();
</script>

<style scoped>
.task-modal {
  width: min(760px, calc(100vw - 32px));
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.form-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.form-error {
  color: #b42318;
  margin: 0;
}
</style>
