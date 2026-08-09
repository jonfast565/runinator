<template>
  <div class="header-panel step-detail">
    <header class="step-detail-header">
      <div class="step-detail-titles">
        <span class="node-kind">workflow header</span>
        <h2>{{ workflows.workflowDraft.name || "Untitled workflow" }}</h2>
      </div>
      <p class="step-headline">
        Declarations that belong to the workflow rather than to any one node. All four compile into
        the WDL header, so they survive a save.
      </p>
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

    <HeaderInterruptsSection />
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
import HeaderInterruptsSection from "./HeaderInterruptsSection.vue";
import HeaderWatchesSection from "./HeaderWatchesSection.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

// re-derive whenever the draft or the graph changes: an interrupt region's validity depends on
// nodes the panel does not own, so a canvas edit can make a declaration valid or broken.
const issues = computed(() => {
  void workflows.headerDraft;
  void workflows.workflowLayoutVersion;
  void workflows.workflowJson;
  return workflows.getHeaderIssues();
});
</script>
