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
      <button type="button" title="string(selection)" @click="emit('transform', 'string')">
        string()
      </button>
      <button type="button" title="json(selection)" @click="emit('transform', 'json')">
        json()
      </button>
      <button type="button" title="selection ?? fallback" @click="emit('transform', 'coalesce')">
        ??
      </button>
      <button type="button" title="selection ++ value" @click="emit('transform', 'concat')">
        ++
      </button>
    </footer>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { ReferenceGroup } from "../../../core/utils/workflow-references";

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

  if (!needle) {
    return props.groups;
  }

  return props.groups
    .map((group) => ({
      title: group.title,
      references: group.references.filter((reference) =>
        reference.label.toLowerCase().includes(needle),
      ),
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
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
}

.reference-search {
  margin: 8px;
  padding: 5px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
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
  color: var(--text-faint);
  font-size: 12px;
}

.reference-group-title {
  position: sticky;
  top: 0;
  padding: 5px 2px;
  background: var(--surface-subtle);
  color: var(--text-muted);
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
  background: var(--surface-hover);
}

.reference-label {
  color: var(--text);
  font-size: 12px;
}

.reference-type {
  flex: 0 0 auto;
  color: var(--text-faint);
  font-size: 11px;
}

.reference-transforms {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 8px;
  border-top: 1px solid var(--border-subtle);
}

.reference-transforms-label {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
}

.reference-transforms button {
  padding: 3px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text-subtle);
  cursor: pointer;
  font-size: 12px;
}

.reference-transforms button:hover {
  background: var(--surface-hover);
}
</style>
