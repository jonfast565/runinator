<template>
  <div class="app-shell" @keydown="handleKeydown" tabindex="0">
    <SidebarNav />
    <section class="workspace">
      <TopToolbar @refresh="refreshActive" />
      <main>
        <slot />
      </main>
      <div v-if="app.serviceBlocked" class="app-loader-overlay">
        <div class="app-loader">
          <div class="app-loader-spinner"></div>
          <p>{{ app.loadingMessage }}</p>
        </div>
      </div>
    </section>
    <ToastHost />
    <TaskEditorModal v-if="tasks.taskEditorOpen" />
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from "../../stores/app";
import { useKeyboardShortcuts } from "../../composables/useKeyboardShortcuts";
import { useTasksStore } from "../../stores/tasks";
import TaskEditorModal from "../shared/TaskEditorModal.vue";
import SidebarNav from "./SidebarNav.vue";
import ToastHost from "./ToastHost.vue";
import TopToolbar from "./TopToolbar.vue";

const app = useAppStore();
const tasks = useTasksStore();
const { handleKeydown, refreshActive } = useKeyboardShortcuts();
</script>
