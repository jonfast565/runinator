<template>
  <div class="modal-backdrop">
    <form class="modal workflow-settings-modal" @submit.prevent="workflows.closeWorkflowSettings">
      <header class="modal-header">
        <h2>Workflow Settings</h2>
        <button type="button" @click="workflows.closeWorkflowSettings">Close</button>
      </header>

      <section class="form-section">
        <div class="form-grid">
          <label>Name <input v-model="workflows.workflowDraft.name" @input="workflows.markWorkflowDirty" /></label>
          <label>Version <input v-model.number="workflows.workflowDraft.version" type="number" min="1" @input="workflows.markWorkflowDirty" /></label>
          <label>Concurrency <input v-model.number="workflows.workflowConcurrency" type="number" min="1" max="256" @input="workflows.markWorkflowDirty" /></label>
          <label class="checkbox"><input v-model="workflows.workflowDraft.enabled" type="checkbox" @change="workflows.markWorkflowDirty" /> Enabled</label>
        </div>
      </section>

      <div class="modal-actions">
        <button type="submit">Done</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";

const workflows = useWorkflowsStore();
</script>
