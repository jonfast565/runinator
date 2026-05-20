<template>
  <div class="workflow-toolbar">
    <div class="workflow-title">
      <strong>{{ workflows.workflowDraft.name }}</strong>
      <span>v{{ workflows.workflowDraft.version }} · concurrency {{ workflows.workflowConcurrency }}</span>
    </div>
    <div class="workflow-actions">
      <button @click="workflows.openWorkflowSettings">Settings</button>
      <button title="Arrange workflow nodes left to right" @click="workflows.autoArrangeWorkflowNodes('horizontal')">Arrange H</button>
      <button title="Arrange workflow nodes top to bottom" @click="workflows.autoArrangeWorkflowNodes('vertical')">Arrange V</button>
      <button @click="workflows.saveSelectedWorkflow">Save</button>
      <button :disabled="!workflows.canRunWorkflow" @click="workflows.runSelectedWorkflow()">Run</button>
      <button
        v-if="!isActiveDebugRun"
        class="run-debug-btn"
        :disabled="!workflows.canRunWorkflow"
        @click="workflows.runSelectedWorkflowDebug"
      >
        🐞 Run Debug
      </button>
      <button
        v-else
        class="stop-debug-btn"
        :disabled="!workflows.canCancelWorkflowRun"
        title="Cancel the active debug run"
        @click="workflows.cancelSelectedWorkflowRun"
      >
        ■ Stop Debug
      </button>
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
import { computed } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";

const workflows = useWorkflowsStore();

const isActiveDebugRun = computed(() => {
  if (!workflows.isDebugRun) return false;
  const status = workflows.workflowRunDetail?.run.status;
  if (!status) return false;
  return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
});
</script>

<style scoped>
.run-debug-btn {
  background: #fef3c7;
  border-color: #f59e0b;
  color: #92400e;
  font-weight: 600;
}
.run-debug-btn:hover:not(:disabled) {
  background: #fde68a;
}
.stop-debug-btn {
  background: #dc2626;
  border-color: #dc2626;
  color: #fff;
  font-weight: 600;
}
.stop-debug-btn:hover:not(:disabled) {
  background: #b91c1c;
}
</style>
