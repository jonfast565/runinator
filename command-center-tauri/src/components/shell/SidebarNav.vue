<template>
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-mark">R</span>
      <span>Command Center</span>
    </div>
    <nav class="nav-list">
      <button v-for="tab in tabs" :key="tab" :class="{ active: app.activeTab === tab }" @click="app.activeTab = tab">
        <span>{{ tab }}</span>
        <span class="nav-count">{{ navCount(tab) }}</span>
      </button>
    </nav>
  </aside>
</template>

<script setup lang="ts">
import { tabs, useAppStore } from "../../stores/app";
import { useResourcesStore } from "../../stores/resources";
import { useTasksStore } from "../../stores/tasks";
import { useWorkflowsStore } from "../../stores/workflows";
import type { AppTab } from "../../types/app";

const app = useAppStore();
const tasks = useTasksStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();

function navCount(tab: AppTab): number {
  if (tab === "Tasks") return tasks.tasks.length;
  if (tab === "Runs") return tasks.runs.length;
  if (tab === "Workflows") return workflows.workflows.length;
  return resources.resourceRecords.length;
}
</script>
