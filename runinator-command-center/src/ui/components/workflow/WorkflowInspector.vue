<template>
  <!-- the step/interrupts/header/wdl views are each their own promoted split tab now, instead of one
       "Inspector" tab hiding all four -- but exactly one is ever open. the other three always sit
       together in a fixed-order rail beside it, rather than nested splits that would scatter folded
       tabs on both sides of whichever one is open. -->
  <div class="workflow-inspector">
    <SplitPane
      class="workflow-inspector-main"
      storage-key="command-center.workflows.inspector-split"
      :initial-first-pct="64"
      :min-first="480"
      :min-second="320"
    >
      <template #first><slot name="canvas" /></template>
      <template #second>
        <div class="panel inspector-panel">
          <StepEditor v-if="activeId === 'step'" />
          <WorkflowInterruptsPanel v-else-if="activeId === 'interrupts'" />
          <WorkflowHeaderPanel v-else-if="activeId === 'header'" />
          <div v-else class="workflow-wdl-pane">
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
        </div>
      </template>
    </SplitPane>

    <nav class="workflow-inspector-rail" role="tablist">
      <button
        v-for="tab in foldedTabs"
        :key="tab.id"
        type="button"
        class="split-tab"
        role="tab"
        :title="tab.title"
        :aria-label="`Show ${tab.title}`"
        @click="openPane(tab.id)"
      >
        <Icon :name="tab.icon" :size="13" />
        <span v-if="tab.badge" class="split-tab-badge">{{ tab.badge }}</span>
        <span class="split-tab-label">{{ tab.label }}</span>
      </button>
    </nav>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { useSecretsStore } from "../../adapters/pinia/secrets";
import Icon, { type IconName } from "../shared/Icon.vue";
import SplitPane from "../shared/SplitPane.vue";
import WdlEditor from "../shared/WdlEditor.vue";
import StepEditor from "./StepEditor.vue";
import WorkflowHeaderPanel from "./WorkflowHeaderPanel.vue";
import WorkflowInterruptsPanel from "./WorkflowInterruptsPanel.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const secretsStore = useSecretsStore();

type InspectorTabId = "step" | "interrupts" | "header" | "wdl";

interface InspectorTab {
  id: InspectorTabId;
  label: string;
  icon: IconName;
  title: string;
  badge?: number;
}

const tabs = computed<InspectorTab[]>(() => [
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

// the mode lives in service state because other actions set it too: clicking a canvas node opens
// back to the step view, and the toolbar's interrupts/header links open theirs.
const activeId = computed<InspectorTabId>(() => workflows.workflowInspectorMode);
const foldedTabs = computed(() => tabs.value.filter((tab) => tab.id !== activeId.value));

function openPane(id: InspectorTabId) {
  if (id === "header") {
    workflows.openWorkflowHeader();
    return;
  }

  if (id === "interrupts") {
    workflows.openWorkflowInterrupts();
    return;
  }

  if (id === "wdl") {
    workflows.openWorkflowWdl();
    return;
  }

  workflows.closeWorkflowHeader();
}
</script>
