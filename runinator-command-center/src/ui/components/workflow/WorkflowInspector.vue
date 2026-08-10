<template>
  <!-- three views docked on the same side: the selected step, the interrupt handlers, and the
       remaining workflow-level header declarations. -->
  <PanelStack
    v-model="inspectorMode"
    class="panel inspector-panel"
    storage-key="command-center.workflows.inspector-stack"
    :tabs="tabs"
  >
    <template #step><StepEditor /></template>
    <template #interrupts><WorkflowInterruptsPanel /></template>
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
import WorkflowInterruptsPanel from "./WorkflowInterruptsPanel.vue";

const workflows = useWorkflowsStore();

const tabs = computed<PanelStackTab[]>(() => [
  { id: "step", label: "Step", icon: "settings", title: "The selected node" },
  {
    id: "interrupts",
    label: "Interrupts",
    icon: "bolt",
    title: "Interrupt handlers and their regions",
    badge: workflows.interruptIssueCount || undefined,
  },
  {
    id: "header",
    label: "Header",
    icon: "flag",
    title: "Watch guards, concurrency, and the correlation key",
    badge: workflows.declarationIssueCount || undefined,
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

    if (mode === "interrupts") {
      workflows.openWorkflowInterrupts();
      return;
    }

    workflows.closeWorkflowHeader();
  },
});
</script>
