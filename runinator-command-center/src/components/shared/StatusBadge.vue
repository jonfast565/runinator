<template>
  <span class="badge" :class="badgeClass">{{ label }}</span>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { statusBadgeClass } from "../../utils/status";

const props = defineProps<{
  status?: string | boolean | null;
  trueLabel?: string;
  falseLabel?: string;
}>();

const label = computed(() => {
  if (typeof props.status === "boolean") {
    return props.status ? (props.trueLabel ?? "Enabled") : (props.falseLabel ?? "Disabled");
  }

  return props.status ?? "-";
});
const badgeClass = computed(() => {
  if (typeof props.status === "boolean") {
    return props.status ? "status-succeeded" : "status-muted";
  }

  return statusBadgeClass(props.status ?? undefined);
});
</script>
