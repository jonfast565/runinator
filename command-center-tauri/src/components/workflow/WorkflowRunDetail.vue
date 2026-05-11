<template>
  <div class="inspector-section">
    <h2>Run Detail</h2>
    <pre class="output workflow-detail">{{ workflows.workflowRunDetailText }}</pre>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";

const workflows = useWorkflowsStore();
</script>
