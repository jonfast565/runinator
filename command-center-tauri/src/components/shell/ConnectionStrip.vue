<template>
  <div class="connection-strip">
    <span class="connection-pill" :class="{ connected: app.serviceConnected, waiting: !app.serviceConnected }">
      {{ app.serviceConnected ? "Connected" : "Waiting for service" }}
    </span>
    <span class="service-url">{{ app.serviceLabel }}</span>
    <span class="last-refresh">Last refresh: {{ app.lastRefreshText }}</span>
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from "../../stores/app";

const app = useAppStore();
</script>
