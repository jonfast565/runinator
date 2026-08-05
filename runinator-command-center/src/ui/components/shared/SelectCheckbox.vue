<template>
  <input
    ref="input"
    type="checkbox"
    class="size-3.5 shrink-0 cursor-pointer accent-[var(--accent)]"
    :checked="checked"
    :aria-label="label"
    @click.stop="onClick"
    @keydown.stop
  />
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from "vue";

// selection checkbox for a table row or header. stops click propagation so ticking a row does not
// also fire the table's row-select (which would navigate away mid-selection).
const props = withDefaults(
  defineProps<{
    checked: boolean;
    label: string;
    // header state: some but not all rows selected.
    indeterminate?: boolean;
  }>(),
  { indeterminate: false },
);

const emit = defineEmits<{ toggle: [event: MouseEvent] }>();

const input = ref<HTMLInputElement | null>(null);

// `indeterminate` is a dom property with no attribute equivalent, so it has to be assigned.
function syncIndeterminate() {
  if (input.value) {
    input.value.indeterminate = props.indeterminate;
  }
}

onMounted(syncIndeterminate);
watch(() => props.indeterminate, syncIndeterminate);

function onClick(event: MouseEvent) {
  // the parent owns selection state; re-sync the dom so the checkbox never drifts from the model.
  const target = event.target as HTMLInputElement;
  target.checked = props.checked;
  emit("toggle", event);
}
</script>
