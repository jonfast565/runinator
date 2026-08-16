<template>
  <div class="flex items-center gap-1 px-3 py-1 font-mono text-[11px]">
    <span class="bg-fg-inverse px-2 font-semibold text-inverse">runinator</span>
    <span class="ml-1 font-semibold text-accent-pulse">{{ session }}</span>
    <span class="text-fg-inverse-faint">·</span>
    <span class="truncate text-fg-inverse-faint">{{ service }}</span>
    <span class="text-fg-inverse-faint">·</span>
    <span :class="busy ? 'text-warning-fg' : 'text-fg-inverse-muted'">{{
      busy ? "running" : "ready"
    }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { getCommandRuntimeOptional } from "../../../core/api/runtime";

defineProps<{ session: string; busy: boolean }>();

// the same three facts the runinatorctl status line carries: which session, which service, and
// whether it is busy. the service is read from the runtime rather than passed in, because it is the
// runtime's answer either way.
const service = computed(() => getCommandRuntimeOptional()?.apiBaseUrl() ?? "not connected");
</script>
