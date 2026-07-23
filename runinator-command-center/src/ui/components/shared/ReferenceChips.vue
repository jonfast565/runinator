<template>
  <div class="reference-chips">
    <p class="reference-chips-hint">
      {{
        insertTarget
          ? "Click to insert into the focused field"
          : "Click to copy — focus an expression field to insert directly"
      }}
    </p>
    <div v-for="group in groups" :key="group.title" class="reference-chip-group">
      <span class="reference-chip-group-title">{{ group.title }}</span>
      <div class="reference-chip-row">
        <!-- mousedown.prevent keeps the focused expression editor from blurring before the click. -->
        <button
          v-for="reference in group.references"
          :key="reference.insert"
          type="button"
          class="reference-chip"
          :class="{ flashed: flashed === reference.insert }"
          :title="reference.insert"
          @mousedown.prevent
          @click="pick(reference)"
        >
          <code class="reference-chip-label">{{ reference.label }}</code>
          <small class="reference-chip-type">{{ reference.type }}</small>
          <span v-if="flashed === reference.insert" class="reference-chip-flag">{{
            flashLabel
          }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import type { ReferenceGroup, WorkflowReference } from "../../../core/utils/workflow-references";
import { useExpressionInsertTarget } from "../../../ui/adapters/codemirror/expression-insert-target";

defineProps<{
  groups: ReferenceGroup[];
}>();

const insertTarget = useExpressionInsertTarget();
const flashed = ref<string | null>(null);
const flashLabel = ref("");
let flashTimer: ReturnType<typeof setTimeout> | null = null;

// insert into the focused expression field if one is active, otherwise fall back to clipboard copy.
function pick(reference: WorkflowReference) {
  const insert = insertTarget.value;

  if (insert) {
    insert(reference.insert);
    flash(reference.insert, "inserted");
    return;
  }

  void navigator.clipboard.writeText(reference.insert).catch(() => undefined);
  flash(reference.insert, "copied");
}

function flash(insert: string, label: string) {
  flashed.value = insert;
  flashLabel.value = label;

  if (flashTimer) {
    clearTimeout(flashTimer);
  }

  flashTimer = setTimeout(() => {
    flashed.value = null;
  }, 900);
}
</script>

