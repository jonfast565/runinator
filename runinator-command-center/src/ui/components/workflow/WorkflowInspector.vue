<template>
  <!-- two views docked on the same side: the selected step, and the workflow-level header. -->
  <PanelStack
    v-model="inspectorMode"
    class="panel inspector-panel"
    storage-key="command-center.workflows.inspector-stack"
    :tabs="tabs"
  >
    <template #step><StepEditor /></template>
    <template #header><WorkflowHeaderPanel /></template>
  </PanelStack>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import PanelStack from "../shared/PanelStack.vue";
import type { PanelStackTab } from "../shared/panel-stack";
import StepEditor from "./StepEditor.vue";
import WorkflowHeaderPanel from "./WorkflowHeaderPanel.vue";

const workflows = useWorkflowsStore();

const tabs = computed<PanelStackTab[]>(() => [
  { id: "step", label: "Step", icon: "settings", title: "The selected node" },
  {
    id: "header",
    label: "Header",
    icon: "flag",
    title: "Interrupts, watch guards, concurrency, and the correlation key",
    badge: workflows.headerIssueCount || undefined,
  },
]);

// the mode lives in service state because other actions set it: clicking a canvas node switches
// back to the step view, so the stack cannot own it privately.
const inspectorMode = computed({
  get: () => workflows.workflowInspectorMode,
  set: (mode) => {
    if (mode === "header") {
      workflows.openWorkflowHeader();
      return;
    }

    workflows.closeWorkflowHeader();
  },
});
</script>
