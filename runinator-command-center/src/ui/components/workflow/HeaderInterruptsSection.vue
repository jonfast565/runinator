<template>
  <section class="detail-section header-section">
    <h3>Interrupt handlers</h3>
    <p class="section-note">
      An interrupt suspends one thread of control, runs a handler region beside it, and hands
      control back at a <code>resume</code>. A region is entered only by its interrupt, so its nodes
      are unreachable from <code>start</code> by design.
    </p>

    <p v-if="declarations.length === 0" class="hint">No interrupt handlers declared.</p>

    <div v-for="(entry, index) in declarations" :key="`${entry.source}-${index}`" class="header-row">
      <div class="header-row-controls">
        <label
          >Source
          <select
            :value="entry.source"
            @change="workflows.setHeaderInterruptSource(index, selectValue($event))"
          >
            <option v-for="option in sourceOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </label>
        <label
          >Handler
          <select
            :value="entry.handler"
            @change="workflows.setHeaderInterruptHandler(index, selectValue($event))"
          >
            <!-- keep the current value selectable even when it is no longer a legal candidate, so
                 the picker shows what is wrong instead of silently rewriting it. -->
            <option v-if="!candidates.includes(entry.handler)" :value="entry.handler">
              {{ entry.handler }} (invalid)
            </option>
            <option v-for="id in candidates" :key="id" :value="id">{{ id }}</option>
          </select>
        </label>
        <button type="button" class="btn btn-sm" @click="workflows.removeHeaderInterrupt(index)">
          Remove
        </button>
      </div>
      <p class="region-chips">
        <span class="region-label">Region</span>
        <button
          v-for="id in workflows.getRegionNodeIds(entry.handler)"
          :key="id"
          type="button"
          class="region-chip"
          @click="selectNode(id)"
        >
          {{ id }}
        </button>
      </p>
      <p class="hint">{{ describeSource(entry.source) }}</p>
    </div>

    <div class="header-actions">
      <select v-model="newSource" :disabled="undeclared.length === 0">
        <option v-for="source in undeclared" :key="source" :value="source">
          {{ labelFor(source) }}
        </option>
      </select>
      <button
        type="button"
        class="btn btn-primary btn-sm"
        :disabled="!newSource || !catalogMetadata.loaded"
        title="Create an audit + resume region and declare it for this source"
        @click="scaffold"
      >
        Add handler
      </button>
      <select v-model="existingHandlerId" :disabled="candidates.length === 0" aria-label="Existing node">
        <option v-for="id in candidates" :key="id" :value="id">{{ id }}</option>
      </select>
      <button
        type="button"
        class="btn btn-sm"
        :disabled="!newSource || !existingHandlerId"
        title="Point this source at the selected region you have already drawn"
        @click="declareExisting"
      >
        Use existing node
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

const declarations = computed(() => workflows.headerDraft.interrupts);
const sourceOptions = computed(() => catalogMetadata.enumOptions("interrupt_source"));
const candidates = computed(() => {
  // the candidate set is a graph property, so it has to re-derive on canvas edits too.
  void workflows.workflowLayoutVersion;
  void workflows.workflowJson;
  return workflows.getHandlerCandidateNodeIds();
});
const undeclared = computed(() =>
  workflows.getUndeclaredInterruptSources(sourceOptions.value.map((option) => option.value)),
);
const newSource = ref("");
const existingHandlerId = ref("");

watch(
  undeclared,
  (sources) => {
    if (!sources.includes(newSource.value)) {
      newSource.value = sources.at(0) ?? "";
    }
  },
  { immediate: true },
);

watch(
  candidates,
  (ids) => {
    if (!ids.includes(existingHandlerId.value)) {
      existingHandlerId.value = ids.at(0) ?? "";
    }
  },
  { immediate: true },
);

function selectValue(event: Event): string {
  return (event.target as HTMLSelectElement).value;
}

function labelFor(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.label ?? source;
}

function describeSource(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.description ?? "";
}

function selectNode(nodeId: string) {
  workflows.populateStepEditor(nodeId);
}

function scaffold() {
  workflows.scaffoldInterruptHandler(newSource.value);
}

function declareExisting() {
  if (existingHandlerId.value) {
    workflows.declareHeaderInterrupt(newSource.value, existingHandlerId.value);
  }
}
</script>
