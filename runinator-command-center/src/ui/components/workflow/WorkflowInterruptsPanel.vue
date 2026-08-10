<template>
  <div class="header-panel step-detail">
    <header class="step-detail-header">
      <div class="step-detail-titles">
        <span class="node-kind">interrupts</span>
        <h2>{{ workflows.workflowDraft.name || "Untitled workflow" }}</h2>
      </div>
      <p class="step-headline">
        An interrupt suspends one thread of control, runs a handler region beside it, and hands
        control back at a <code>resume</code>. Adding a handler creates the complete region and opens
        its first editable step; extend that mini-flow on the canvas like any other sequence.
      </p>
    </header>

    <section v-if="issues.length" class="detail-section validation-section">
      <h3>Interrupt validation</h3>
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
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import HeaderInterruptsSection from "./HeaderInterruptsSection.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

// re-derive whenever the draft or the graph changes: a region's validity depends on nodes this
// panel does not own, so a canvas edit can make a declaration valid or broken.
const issues = computed(() => {
  void workflows.headerDraft;
  void workflows.workflowLayoutVersion;
  void workflows.workflowJson;
  return workflows.getInterruptIssues();
});
</script>
