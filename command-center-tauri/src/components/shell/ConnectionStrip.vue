<template>
  <div class="connection-strip">
    <span class="connection-pill" :class="{ connected: Boolean(app.serviceUrl), waiting: !app.serviceUrl }">
      {{ app.serviceUrl ? "Connected" : "Waiting for service" }}
    </span>
    <span class="service-url">{{ app.serviceUrl ?? "No service discovered" }}</span>
    <span class="last-refresh">Last refresh: {{ app.lastRefreshText }}</span>
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from "../../stores/app";

const app = useAppStore();
</script>
