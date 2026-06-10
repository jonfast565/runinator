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
          <label>Version <input v-model="workflows.workflowDraft.version" type="text" placeholder="1.0.0" pattern="\d+\.\d+\.\d+" @input="workflows.markWorkflowDirty" /></label>
          <label>Concurrency <input v-model.number="workflows.workflowConcurrency" type="number" min="1" max="256" @input="workflows.markWorkflowDirty" /></label>
          <label class="checkbox"><input v-model="workflows.workflowDraft.enabled" type="checkbox" @change="workflows.markWorkflowDirty" /> Enabled</label>
        </div>
      </section>

      <section class="form-section trigger-section">
        <div class="section-toolbar">
          <h3>Triggers</h3>
          <div class="section-actions">
            <button type="button" :disabled="!workflows.canManageWorkflowTriggers" @click="workflows.refreshWorkflowTriggers">Refresh</button>
            <button type="button" :disabled="!workflows.canManageWorkflowTriggers" @click="workflows.addWorkflowTrigger('cron')">New Cron</button>
            <button type="button" :disabled="!workflows.canManageWorkflowTriggers" @click="workflows.addWorkflowTrigger('manual')">New Manual</button>
          </div>
        </div>

        <p v-if="!workflows.canManageWorkflowTriggers" class="hint">Save the workflow before adding triggers.</p>
        <p v-else-if="workflows.workflowTriggers.length === 0" class="hint">No triggers configured.</p>

        <div v-else class="trigger-table-wrap">
          <table class="compact">
            <thead>
              <tr>
                <th>Kind</th>
                <th>State</th>
                <th>Schedule</th>
                <th>Next</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="trigger in workflows.workflowTriggers" :key="trigger.id ?? `${trigger.kind}-${trigger.workflow_id}`" :class="{ muted: !trigger.enabled }">
                <td>{{ trigger.kind }}</td>
                <td>{{ trigger.enabled ? "enabled" : "disabled" }}</td>
                <td>{{ workflows.triggerCronSummary(trigger) || "-" }}</td>
                <td>{{ trigger.next_execution ?? "-" }}</td>
                <td class="row-actions">
                  <button type="button" @click="workflows.editWorkflowTrigger(trigger)">Edit</button>
                  <button type="button" @click="workflows.deleteSelectedWorkflowTrigger(trigger)">Delete</button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div v-if="workflows.triggerEditorOpen" class="trigger-editor">
          <div class="section-toolbar">
            <h3>{{ workflows.triggerEditorCreating ? "New Trigger" : "Edit Trigger" }}</h3>
            <button type="button" @click="workflows.closeTriggerEditor">Cancel</button>
          </div>
          <div class="form-grid">
            <label>
              Kind
              <select v-model="workflows.triggerDraft.kind" @change="workflows.setTriggerKind(workflows.triggerDraft.kind)">
                <option value="cron">cron</option>
                <option value="manual">manual</option>
              </select>
            </label>
            <label class="checkbox"><input v-model="workflows.triggerDraft.enabled" type="checkbox" /> Enabled</label>
            <label>Next Execution <input v-model="workflows.triggerDraft.next_execution" type="datetime-local" /></label>
            <label>Blackout Start <input v-model="workflows.triggerDraft.blackout_start" type="datetime-local" /></label>
            <label>Blackout End <input v-model="workflows.triggerDraft.blackout_end" type="datetime-local" /></label>
          </div>
          <div class="trigger-json-grid">
            <div class="form-field">
              <span class="form-field-label">Configuration</span>
              <JsonEditor v-model="workflows.triggerJson.configuration" />
            </div>
            <div class="form-field">
              <span class="form-field-label">Metadata</span>
              <JsonEditor v-model="workflows.triggerJson.metadata" />
            </div>
          </div>
          <p v-if="workflows.triggerEditorError" class="form-error">{{ workflows.triggerEditorError }}</p>
          <div class="modal-actions">
            <button type="button" @click="workflows.closeTriggerEditor">Cancel</button>
            <button type="button" @click="workflows.submitWorkflowTrigger">Save Trigger</button>
          </div>
        </div>
      </section>

      <div class="modal-actions">
        <button type="button" class="danger" :disabled="!workflows.workflowDraft.id" @click="workflows.deleteSelectedWorkflow">Delete Workflow</button>
        <button type="button" :disabled="!workflows.workflowDraft.id || workflows.isDirty" @click="workflows.duplicateSelectedWorkflow('minor')">Duplicate (bump version)</button>
        <button type="submit">Done</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";

const workflows = useWorkflowsStore();
</script>

<style scoped>
.workflow-settings-modal {
  width: min(1040px, 100%);
}

.section-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.section-actions,
.row-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.hint,
.form-error {
  margin: 0;
  font-size: 12px;
}

.hint {
  color: #66717e;
}

.form-error {
  color: #a33a2f;
}

.danger {
  border-color: #c94b3f;
  color: #a33a2f;
}

.trigger-table-wrap {
  overflow: auto;
  border: 1px solid #edf1f5;
  border-radius: 6px;
}

.trigger-table-wrap th:last-child,
.trigger-table-wrap td:last-child {
  width: 148px;
}

.trigger-editor {
  display: grid;
  gap: 12px;
  border-top: 1px solid #e5ebf1;
  padding-top: 12px;
}

.trigger-json-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

@media (max-width: 760px) {
  .section-toolbar,
  .trigger-json-grid {
    grid-template-columns: 1fr;
  }

  .section-toolbar {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
