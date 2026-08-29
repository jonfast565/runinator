<template>
  <div class="header-panel step-detail">
    <header class="step-detail-header">
      <div class="step-detail-titles">
        <span class="node-kind">workflow header</span>
        <div class="flex items-center gap-1">
          <h2>{{ workflows.workflowDraft.name || "Untitled workflow" }}</h2>
          <HelpBubble
            text="Declarations that belong to the workflow rather than any one node. They compile into the REXRAP header and survive a save. Interrupt handlers have their own panel."
            label="About the workflow header"
          />
        </div>
      </div>
    </header>

    <section v-if="issues.length" class="detail-section validation-section">
      <h3>Header validation</h3>
      <div class="detail-rows">
        <div
          v-for="issue in issues"
          :key="`${issue.severity}-${issue.nodeId}-${issue.message}`"
          class="detail-row"
          :class="`issue-${issue.severity}`"
        >
          <span>{{ issue.severity }}</span>
          <strong>{{ issue.message }}</strong>
        </div>
      </div>
    </section>

    <p v-if="!catalogMetadata.loaded" class="hint catalog-loading-hint">
      <LoadingSpinner size="sm" label="Loading node types" />
      Loading node types…
    </p>

    <HeaderWatchesSection />
    <HeaderConcurrencySection />
    <HeaderCorrelationSection />
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import HeaderConcurrencySection from "./HeaderConcurrencySection.vue";
import HeaderCorrelationSection from "./HeaderCorrelationSection.vue";
import HeaderWatchesSection from "./HeaderWatchesSection.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import HelpBubble from "../shared/HelpBubble.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

// re-derive whenever the draft or the graph changes: a watch guard's handler is a node this panel
// does not own, so a canvas edit can make a declaration valid or broken.
const issues = computed(() => {
  void workflows.headerDraft;
  void workflows.workflowLayoutVersion;
  void workflows.workflowJson;
  return workflows.getDeclarationIssues();
});
</script>
