<template>
  <section class="reference-picker">
    <input
      ref="searchInput"
      v-model="query"
      class="reference-search"
      type="text"
      placeholder="Filter references…"
      spellcheck="false"
    />
    <div class="reference-groups">
      <p v-if="filteredGroups.length === 0" class="reference-empty">No references in scope.</p>
      <div v-for="group in filteredGroups" :key="group.title" class="reference-group">
        <header class="reference-group-title">{{ group.title }}</header>
        <button
          v-for="reference in group.references"
          :key="reference.insert"
          type="button"
          class="reference-row"
          :title="`Insert ${reference.insert}`"
          @click="emit('insert', reference.insert)"
        >
          <code class="reference-label">{{ reference.label }}</code>
          <small class="reference-type">{{ reference.type }}</small>
        </button>
      </div>
    </div>
    <footer class="reference-transforms">
      <span class="reference-transforms-label">Wrap selection</span>
      <button type="button" @click="emit('transform', 'string')" title="string(selection)">string()</button>
      <button type="button" @click="emit('transform', 'json')" title="json(selection)">json()</button>
      <button type="button" @click="emit('transform', 'coalesce')" title="selection ?? fallback">??</button>
      <button type="button" @click="emit('transform', 'concat')" title="selection ++ value">++</button>
    </footer>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { ReferenceGroup } from "../../utils/workflow-references";

const props = defineProps<{
  groups: ReferenceGroup[];
}>();

const emit = defineEmits<{
  insert: [text: string];
  transform: [kind: "string" | "json" | "coalesce" | "concat"];
}>();

const query = ref("");

// filter references case-insensitively by label, dropping groups left empty.
const filteredGroups = computed<ReferenceGroup[]>(() => {
  const needle = query.value.trim().toLowerCase();
  if (!needle) return props.groups;
  return props.groups
    .map((group) => ({
      title: group.title,
      references: group.references.filter((reference) => reference.label.toLowerCase().includes(needle))
    }))
    .filter((group) => group.references.length > 0);
});
</script>

<style scoped>
.reference-picker {
  display: flex;
  flex-direction: column;
  min-height: 0;
  max-height: 280px;
  border-top: 1px solid #e3e8ee;
  background: #fbfcfe;
}

.reference-search {
  margin: 8px;
  padding: 5px 8px;
  border: 1px solid #ccd4dd;
  border-radius: 5px;
  font-size: 12px;
}

.reference-groups {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  padding: 0 8px 8px;
}

.reference-empty {
  margin: 4px 2px;
  color: #8a949f;
  font-size: 12px;
}

.reference-group-title {
  position: sticky;
  top: 0;
  padding: 5px 2px;
  background: #fbfcfe;
  color: #66717e;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.reference-row {
  display: flex;
  width: 100%;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  padding: 4px 6px;
  border: none;
  border-radius: 4px;
  background: transparent;
  cursor: pointer;
  text-align: left;
}

.reference-row:hover {
  background: #eef3f9;
}

.reference-label {
  color: #2f3a45;
  font-size: 12px;
}

.reference-type {
  flex: 0 0 auto;
  color: #8a949f;
  font-size: 11px;
}

.reference-transforms {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 8px;
  border-top: 1px solid #e3e8ee;
}

.reference-transforms-label {
  color: #66717e;
  font-size: 11px;
  font-weight: 700;
}

.reference-transforms button {
  padding: 3px 8px;
  border: 1px solid #ccd4dd;
  border-radius: 4px;
  background: #fff;
  color: #3b4652;
  cursor: pointer;
  font-size: 12px;
}

.reference-transforms button:hover {
  background: #eef3f9;
}
</style>
