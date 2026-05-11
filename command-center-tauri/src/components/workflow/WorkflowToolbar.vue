<template>
  <div class="workflow-toolbar">
    <div class="workflow-title">
      <strong>{{ workflows.workflowDraft.name }}</strong>
      <span>v{{ workflows.workflowDraft.version }} · concurrency {{ workflows.workflowConcurrency }}</span>
    </div>
    <button @click="workflows.openWorkflowSettings">Settings</button>
    <button @click="workflows.saveSelectedWorkflow">Save</button>
    <button @click="workflows.addWorkflowStep">Add Step</button>
    <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Remove Step</button>
    <WorkflowSettingsModal v-if="workflows.workflowSettingsOpen" />
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";

const workflows = useWorkflowsStore();
</script>
