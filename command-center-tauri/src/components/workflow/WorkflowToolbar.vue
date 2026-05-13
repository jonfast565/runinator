<template>
  <div class="workflow-toolbar">
    <div class="workflow-title">
      <strong>{{ workflows.workflowDraft.name }}</strong>
      <span>v{{ workflows.workflowDraft.version }} · concurrency {{ workflows.workflowConcurrency }}</span>
    </div>
    <div class="workflow-actions">
      <button @click="workflows.openWorkflowSettings">Settings</button>
      <button title="Auto arrange workflow nodes" @click="workflows.autoArrangeWorkflowNodes">Arrange</button>
      <button @click="workflows.saveSelectedWorkflow">Save</button>
      <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Remove</button>
    </div>
    <div class="workflow-palette" aria-label="Workflow node palette">
      <button v-for="kind in workflows.workflowNodeKinds" :key="kind" :title="`Add ${kind} node`" @click="workflows.addWorkflowNode(kind)">
        {{ kind }}
      </button>
    </div>
    <WorkflowSettingsModal v-if="workflows.workflowSettingsOpen" />
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";

const workflows = useWorkflowsStore();
</script>
