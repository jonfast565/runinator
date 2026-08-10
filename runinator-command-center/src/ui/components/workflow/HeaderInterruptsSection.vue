<template>
  <section class="detail-section header-section">
    <h3>Interrupt handlers</h3>
    <p class="section-note">
      An interrupt suspends one thread of control, runs a handler region beside it, and hands
      control back at a <code>resume</code>. A region is entered only by its interrupt, so its nodes
      are unreachable from <code>start</code> by design.
    </p>

    <p v-if="declarations.length === 0" class="hint">No interrupt handlers declared.</p>

    <div
      v-for="(entry, index) in declarations"
      :key="`${entry.source}-${index}`"
      class="header-row"
      :class="{ 'header-row-disabled': !entry.enabled }"
    >
      <div class="header-row-controls">
        <label class="checkbox interrupt-enabled-toggle">
          <input
            type="checkbox"
            :checked="entry.enabled"
            @change="workflows.setHeaderInterruptEnabled(index, checkedValue($event))"
          />
          Enabled
        </label>
        <label
          >Source
          <select
            :value="entry.source"
            @change="workflows.setHeaderInterruptSource(index, selectValue($event))"
          >
            <option v-for="option in availableSources(entry.source)" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </label>
        <label
          >Linked entry
          <span class="handler-link-control">
            <code>{{ entry.handler }}</code>
            <button type="button" class="region-chip" @click="editEntry(entry.handler)">
              View or rename
            </button>
          </span>
        </label>
        <button type="button" class="btn btn-sm" @click="workflows.removeHeaderInterrupt(index)">
          Delete handler
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
      <label class="header-action-field">
        New handler source
        <select v-model="newSource" :disabled="undeclared.length === 0">
          <option v-for="source in undeclared" :key="source" :value="source">
            {{ labelFor(source) }}
          </option>
        </select>
      </label>
      <button
        type="button"
        class="btn btn-primary btn-sm"
        :disabled="!newSource || !catalogMetadata.loaded"
        title="Create a complete handler region and edit its first step"
        @click="scaffold"
      >
        Add handler
      </button>
      <span v-if="undeclared.length === 0" class="hint">Every interrupt source has a handler.</span>
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
const undeclared = computed(() =>
  workflows.getUndeclaredInterruptSources(sourceOptions.value.map((option) => option.value)),
);
const newSource = ref("");

watch(
  undeclared,
  (sources) => {
    if (!sources.includes(newSource.value)) {
      newSource.value = sources.at(0) ?? "";
    }
  },
  { immediate: true },
);

function selectValue(event: Event): string {
  return (event.target as HTMLSelectElement).value;
}

function checkedValue(event: Event): boolean {
  return (event.target as HTMLInputElement).checked;
}

function labelFor(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.label ?? source;
}

function describeSource(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.description ?? "";
}

function availableSources(current: string) {
  const allowed = new Set([current, ...undeclared.value]);
  return sourceOptions.value.filter((option) => allowed.has(option.value));
}

function selectNode(nodeId: string) {
  workflows.populateStepEditor(nodeId);
}

function editEntry(nodeId: string) {
  workflows.openStepEditor(nodeId);
}

function scaffold() {
  workflows.scaffoldInterruptHandler(newSource.value);
}
</script>
