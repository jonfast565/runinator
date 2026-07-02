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

<style scoped>
.reference-chips {
  display: grid;
  gap: 10px;
}

.reference-chips-hint {
  margin: 0;
  color: #8a949f;
  font-size: 12px;
}

.reference-chip-group {
  display: grid;
  gap: 5px;
}

.reference-chip-group-title {
  color: #66717e;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.reference-chip-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.reference-chip {
  display: inline-flex;
  align-items: baseline;
  gap: 7px;
  padding: 4px 10px;
  border: 1px solid #d4dce5;
  border-radius: 999px;
  background: #f6f9fc;
  cursor: pointer;
  font: inherit;
  line-height: 1.4;
  transition:
    background 0.12s ease,
    border-color 0.12s ease,
    transform 0.12s ease;
}

.reference-chip:hover {
  background: #e9f1fb;
  border-color: #9db8d6;
}

.reference-chip:active {
  transform: translateY(1px);
}

.reference-chip.flashed {
  background: #e4f5ea;
  border-color: #8ecfa6;
}

.reference-chip-label {
  color: #1f2b38;
  font-size: 12px;
  font-weight: 600;
}

.reference-chip-type {
  color: #93a0ad;
  font-size: 11px;
}

.reference-chip-flag {
  color: #2f8a52;
  font-size: 11px;
  font-weight: 700;
}
</style>
