<template>
  <div class="panel inspector-panel">
    <div class="workflow-mode-tabs">
      <button :class="{ active: workflows.workflowInspectorMode === 'step' }" @click="workflows.workflowInspectorMode = 'step'">Step</button>
      <button :class="{ active: workflows.workflowInspectorMode === 'runs' }" @click="workflows.workflowInspectorMode = 'runs'">Runs</button>
      <button :class="{ active: workflows.workflowInspectorMode === 'detail' }" @click="workflows.workflowInspectorMode = 'detail'">Detail</button>
    </div>

    <StepEditor v-show="workflows.workflowInspectorMode === 'step'" />

    <div v-show="workflows.workflowInspectorMode === 'runs'" class="inspector-section">
      <h2>Run History</h2>
      <RunTable :runs="workflows.recentWorkflowRuns" :selected-run-id="workflows.selectedWorkflowRunId" compact @select="workflows.selectWorkflowRun" />
    </div>

    <WorkflowRunDetail v-show="workflows.workflowInspectorMode === 'detail'" />
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import RunTable from "../shared/RunTable.vue";
import StepEditor from "./StepEditor.vue";
import WorkflowRunDetail from "./WorkflowRunDetail.vue";

const workflows = useWorkflowsStore();
</script>
