<template>
  <div v-if="app.statusText || app.errorText" class="toast" :class="{ error: Boolean(app.errorText) }">
    {{ app.statusLine }}
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from "../../stores/app";

const app = useAppStore();
</script>
