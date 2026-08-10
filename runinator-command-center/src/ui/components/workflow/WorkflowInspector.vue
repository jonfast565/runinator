<template>
  <!-- four views docked on the same side: the selected step, the interrupt handlers, the
       remaining workflow-level header declarations, and the wdl source. -->
  <PanelStack
    v-model="inspectorMode"
    class="panel inspector-panel"
    storage-key="command-center.workflows.inspector-stack"
    :tabs="tabs"
  >
    <template #step><StepEditor /></template>
    <template #interrupts><WorkflowInterruptsPanel /></template>
    <template #header><WorkflowHeaderPanel /></template>
    <template #wdl>
      <div class="workflow-wdl-pane">
        <div v-if="workflows.workflowWdlError" class="workflow-wdl-error">
          <Icon name="alert" :size="14" class="workflow-wdl-error-icon" />
          <div class="workflow-wdl-error-body">
            <strong>WDL view paused — the graph isn't well-formed yet.</strong>
            <span>{{ workflows.workflowWdlError }}</span>
            <span class="workflow-wdl-error-hint">
              Fix the issues in the Diagnostics panel on the canvas; the WDL editor re-enables
              automatically once the graph compiles.
            </span>
          </div>
        </div>
        <WdlEditor
          v-model="workflows.workflowWdl"
          class="workflow-wdl-editor"
          :readonly="Boolean(workflows.workflowWdlError)"
          :providers="providersStore.providers"
          :settings="secretsStore.secrets"
        />
      </div>
    </template>
  </PanelStack>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { useSecretsStore } from "../../adapters/pinia/secrets";
import Icon from "../shared/Icon.vue";
import PanelStack from "../shared/PanelStack.vue";
import type { PanelStackTab } from "../shared/panel-stack";
import WdlEditor from "../shared/WdlEditor.vue";
import StepEditor from "./StepEditor.vue";
import WorkflowHeaderPanel from "./WorkflowHeaderPanel.vue";
import WorkflowInterruptsPanel from "./WorkflowInterruptsPanel.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const secretsStore = useSecretsStore();

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
  { id: "wdl", label: "WDL", icon: "file", title: "The workflow's wdl source" },
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

    if (mode === "wdl") {
      workflows.openWorkflowWdl();
      return;
    }

    workflows.closeWorkflowHeader();
  },
});
</script>
