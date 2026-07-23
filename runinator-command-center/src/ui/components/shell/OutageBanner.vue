<template>
  <transition name="outage">
    <div
      v-if="app.showOutageBanner"
      class="flex items-center gap-2.5 border-b border-danger-fg/50 bg-danger-bg px-3.5 py-2 text-[13px] font-medium text-danger-fg"
      role="alert"
    >
      <span
        class="inline-flex size-7 shrink-0 items-center justify-center rounded-md bg-danger/15 text-danger-fg"
      >
        <Icon name="alert" :size="15" />
      </span>
      <span class="min-w-0 flex-1">
        Runinator service is unreachable right now. Retrying automatically&mdash;changes may not be
        saved until it reconnects.
      </span>
      <button
        class="inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-sm border-0 bg-transparent p-0 text-inherit opacity-70 hover:opacity-100"
        aria-label="Dismiss"
        @click="app.dismissOutageBanner()"
      >
        <Icon name="close" :size="13" />
      </button>
    </div>
  </transition>
</template>

<script setup lang="ts">
import Icon from "../shared/Icon.vue";
import { useAppStore } from "../../../ui/adapters/pinia/app";

const app = useAppStore();
</script>
