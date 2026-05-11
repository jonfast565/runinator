<template>
  <div class="app-shell" @keydown="handleKeydown" tabindex="0">
    <SidebarNav />
    <section class="workspace">
      <TopToolbar @refresh="refreshActive" />
      <ConnectionStrip />
      <main>
        <slot />
      </main>
    </section>
    <ToastHost />
    <TaskEditorModal v-if="tasks.taskEditorOpen" />
  </div>
</template>

<script setup lang="ts">
import { useKeyboardShortcuts } from "../../composables/useKeyboardShortcuts";
import { useTasksStore } from "../../stores/tasks";
import TaskEditorModal from "../shared/TaskEditorModal.vue";
import ConnectionStrip from "./ConnectionStrip.vue";
import SidebarNav from "./SidebarNav.vue";
import ToastHost from "./ToastHost.vue";
import TopToolbar from "./TopToolbar.vue";

const tasks = useTasksStore();
const { handleKeydown, refreshActive } = useKeyboardShortcuts();
</script>
