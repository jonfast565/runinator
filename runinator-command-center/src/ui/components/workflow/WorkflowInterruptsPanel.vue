<template>
  <div class="header-panel step-detail">
    <header class="step-detail-header">
      <div class="step-detail-titles">
        <span class="node-kind">interrupts</span>
        <div class="flex items-center gap-1">
          <h2 class="interrupt-workflow-name">
            {{ workflows.workflowDraft.name || "Untitled workflow" }}
          </h2>
          <HelpBubble
            text="An interrupt suspends one thread of control, runs a handler region beside it, and hands control back at a resume. Adding a handler creates the complete region and opens its first editable step; extend that mini-flow on the canvas like any other sequence."
            label="About workflow interrupts"
          />
        </div>
      </div>
    </header>

    <p v-if="!catalogMetadata.loaded" class="hint catalog-loading-hint">
      <LoadingSpinner size="sm" label="Loading node types" />
      Loading node types…
    </p>

    <HeaderInterruptsSection />
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import HeaderInterruptsSection from "./HeaderInterruptsSection.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
</script>
