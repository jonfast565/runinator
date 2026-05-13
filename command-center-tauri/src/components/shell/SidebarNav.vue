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
import { useSecretsStore } from "../../stores/secrets";
import { useTasksStore } from "../../stores/tasks";
import { useWorkflowsStore } from "../../stores/workflows";
import type { AppTab } from "../../types/app";

const app = useAppStore();
const tasks = useTasksStore();
const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const secrets = useSecretsStore();

function navCount(tab: AppTab): number {
  if (tab === "Tasks") return tasks.scheduledTasks.length;
  if (tab === "Runs") return tasks.runs.length;
  if (tab === "Workflows") return workflows.workflows.length;
  if (tab === "Resources") return resources.resourceRecords.length;
  return secrets.secrets.length;
}
</script>
