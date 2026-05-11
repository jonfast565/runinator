<template>
  <div class="modal-backdrop">
    <form class="modal task-modal" @submit.prevent="tasks.submitTask">
      <header class="modal-header">
        <h2>{{ tasks.editingTaskCreating ? "New Task" : "Edit Task" }}</h2>
        <button type="button" @click="tasks.closeTaskEditor">Close</button>
      </header>

      <section class="form-section">
        <h3>Identity</h3>
        <div class="form-grid">
          <label>Name <input v-model="tasks.taskDraft.name" /></label>
          <label>Action Name <input v-model="tasks.taskDraft.action_name" /></label>
          <label>Action Function <input v-model="tasks.taskDraft.action_function" /></label>
          <label class="checkbox"><input v-model="tasks.taskDraft.mcp_enabled" type="checkbox" /> MCP Enabled</label>
        </div>
      </section>

      <section class="form-section">
        <h3>Schedule</h3>
        <div class="form-grid">
          <label>Cron <input v-model="tasks.taskDraft.cron_schedule" /></label>
          <label>Timeout <input v-model.number="tasks.taskDraft.timeout" type="number" min="1" /></label>
          <label class="checkbox"><input v-model="tasks.taskDraft.enabled" type="checkbox" /> Enabled</label>
        </div>
      </section>

      <section class="form-section">
        <h3>Action</h3>
        <label>Configuration <textarea v-model="tasks.taskDraft.action_configuration"></textarea></label>
      </section>

      <section class="form-section json-section">
        <h3>JSON Payloads</h3>
        <label>Input Schema <JsonEditor v-model="tasks.taskJson.input_schema" /></label>
        <label>Default Parameters <JsonEditor v-model="tasks.taskJson.default_parameters" /></label>
        <label>Metadata <JsonEditor v-model="tasks.taskJson.metadata" /></label>
      </section>

      <p v-if="tasks.taskEditorError" class="error">{{ tasks.taskEditorError }}</p>
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
