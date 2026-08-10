<template>
  <!-- the four views that used to live inside one "Inspector" tab stack are now promoted to their own
       split tabs, chained off the canvas: each folds to its own rail tab independently, so more than
       one can stay open at once. -->
  <SplitPane
    ref="wdlSplit"
    storage-key="command-center.workflows.inspector-split.wdl"
    :initial-first-pct="78"
    :min-first="360"
    :min-second="320"
    collapsible-second
    initial-collapsed="second"
    second-label="WDL"
    second-icon="file"
  >
    <template #first>
      <SplitPane
        ref="headerSplit"
        storage-key="command-center.workflows.inspector-split.header"
        :initial-first-pct="76"
        :min-first="360"
        :min-second="300"
        collapsible-second
        initial-collapsed="second"
        second-label="Header"
        second-icon="flag"
        :second-badge="workflows.declarationIssueCount || undefined"
      >
        <template #first>
          <SplitPane
            ref="interruptsSplit"
            storage-key="command-center.workflows.inspector-split.interrupts"
            :initial-first-pct="74"
            :min-first="360"
            :min-second="300"
            collapsible-second
            initial-collapsed="second"
            second-label="Interrupts"
            second-icon="bolt"
            :second-badge="workflows.interruptIssueCount || undefined"
          >
            <template #first>
              <SplitPane
                ref="stepSplit"
                storage-key="command-center.workflows.inspector-split.step"
                :initial-first-pct="64"
                :min-first="480"
                :min-second="320"
                collapsible-second
                second-label="Step"
                second-icon="settings"
              >
                <template #first><slot name="canvas" /></template>
                <template #second><StepEditor /></template>
              </SplitPane>
            </template>
            <template #second><WorkflowInterruptsPanel /></template>
          </SplitPane>
        </template>
        <template #second><WorkflowHeaderPanel /></template>
      </SplitPane>
    </template>
    <template #second>
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
  </SplitPane>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { useSecretsStore } from "../../adapters/pinia/secrets";
import Icon from "../shared/Icon.vue";
import SplitPane from "../shared/SplitPane.vue";
import WdlEditor from "../shared/WdlEditor.vue";
import StepEditor from "./StepEditor.vue";
import WorkflowHeaderPanel from "./WorkflowHeaderPanel.vue";
import WorkflowInterruptsPanel from "./WorkflowInterruptsPanel.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const secretsStore = useSecretsStore();

interface Expandable {
  expand: () => void;
}

const stepSplit = ref<Expandable | null>(null);
const interruptsSplit = ref<Expandable | null>(null);
const headerSplit = ref<Expandable | null>(null);
const wdlSplit = ref<Expandable | null>(null);

// other actions still ask for a mode by name -- a canvas node click, the toolbar's interrupts/header
// links, or a diagnostics click on the canvas. each now only surfaces that one split tab instead of
// switching the whole stack to it, so the rest can stay open alongside it.
watch(
  () => workflows.workflowInspectorMode,
  (mode) => {
    const target = { step: stepSplit, interrupts: interruptsSplit, header: headerSplit, wdl: wdlSplit }[
      mode
    ];
    target.value?.expand();
  },
);
</script>
