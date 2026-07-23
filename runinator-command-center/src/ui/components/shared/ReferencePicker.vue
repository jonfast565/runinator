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

