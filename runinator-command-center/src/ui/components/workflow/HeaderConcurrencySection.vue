<template>
  <section class="detail-section header-section">
    <div class="flex items-center gap-1">
      <h3>Concurrency</h3>
      <HelpBubble label="About workflow concurrency">
        Checked inside the trigger-firing transaction. A manually started run is
        <strong>not</strong> gated by this.
      </HelpBubble>
    </div>

    <p v-if="!concurrency" class="hint">
      Unlimited: overlapping runs are allowed and no firing is ever declined.
    </p>

    <div class="header-row-controls">
      <label
        >Max concurrent runs
        <input
          type="number"
          min="1"
          max="256"
          :value="concurrency?.maxConcurrentRuns ?? 1"
          @change="setMax"
        />
      </label>
      <div class="flex items-end gap-1">
        <label
          >On conflict
          <select :value="concurrency?.onConflict ?? 'skip'" @change="setPolicy">
            <option v-for="option in policyOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </label>
        <HelpBubble
          v-if="policyDescription"
          :text="policyDescription"
          label="About the selected conflict policy"
          :size="14"
        />
      </div>
      <button
        v-if="concurrency"
        type="button"
        class="btn btn-sm"
        title="Remove the header; the workflow becomes unlimited"
        @click="workflows.clearHeaderConcurrency"
      >
        Clear
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import HelpBubble from "../shared/HelpBubble.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

const concurrency = computed(() => workflows.headerDraft.concurrency);
const policyOptions = computed(() => catalogMetadata.enumOptions("concurrency_policy"));
const policyDescription = computed(
  () =>
    policyOptions.value.find((option) => option.value === concurrency.value?.onConflict)
      ?.description ?? "",
);

function setMax(event: Event) {
  const value = Number((event.target as HTMLInputElement).value);
  workflows.setHeaderConcurrency({ maxConcurrentRuns: Number.isFinite(value) ? value : 1 });
}

function setPolicy(event: Event) {
  workflows.setHeaderConcurrency({ onConflict: (event.target as HTMLSelectElement).value });
}
</script>
