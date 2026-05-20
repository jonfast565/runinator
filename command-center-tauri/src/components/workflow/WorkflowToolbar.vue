<template>
  <div ref="toolbarRef" class="workflow-toolbar">
    <div class="workflow-title">
      <strong>{{ workflows.workflowDraft.name }}</strong>
      <span>v{{ workflows.workflowDraft.version }} · concurrency {{ workflows.workflowConcurrency }}</span>
    </div>
    <div class="workflow-actions">
      <button @click="workflows.openWorkflowSettings">Settings</button>
      <div class="toolbar-menu">
        <button
          type="button"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'nodes'"
          @click="toggleMenu('nodes')"
        >
          New Node
        </button>
        <div v-if="openMenu === 'nodes'" class="toolbar-menu-panel" role="menu">
          <button
            v-for="kind in workflows.workflowNodeKinds"
            :key="kind"
            type="button"
            role="menuitem"
            :title="`Add ${kind} node`"
            @click="addNode(kind)"
          >
            {{ kind }}
          </button>
        </div>
      </div>
      <div class="toolbar-menu">
        <button
          type="button"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'arrange'"
          @click="toggleMenu('arrange')"
        >
          Arrange
        </button>
        <div v-if="openMenu === 'arrange'" class="toolbar-menu-panel" role="menu">
          <button type="button" role="menuitem" title="Arrange workflow nodes left to right" @click="arrangeNodes('horizontal')">
            Left to right
          </button>
          <button type="button" role="menuitem" title="Arrange workflow nodes top to bottom" @click="arrangeNodes('vertical')">
            Top to bottom
          </button>
        </div>
      </div>
      <button @click="workflows.saveSelectedWorkflow">Save</button>
      <button :disabled="!workflows.canRunWorkflow" @click="workflows.runSelectedWorkflow()">Run</button>
      <button
        v-if="!isActiveDebugRun"
        class="run-debug-btn"
        :disabled="!workflows.canRunWorkflow"
        @click="workflows.runSelectedWorkflowDebug"
      >
        🐞 Run Debug
      </button>
      <button
        v-else
        class="stop-debug-btn"
        :disabled="!workflows.canCancelWorkflowRun"
        title="Cancel the active debug run"
        @click="workflows.cancelSelectedWorkflowRun"
      >
        ■ Stop Debug
      </button>
      <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Remove</button>
    </div>
    <WorkflowSettingsModal v-if="workflows.workflowSettingsOpen" />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { WorkflowLayoutDirection, WorkflowNodeKind } from "../../types/models";
import { useWorkflowsStore } from "../../stores/workflows";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";

const workflows = useWorkflowsStore();
const toolbarRef = ref<HTMLElement | null>(null);
const openMenu = ref<"nodes" | "arrange" | null>(null);

const isActiveDebugRun = computed(() => {
  if (!workflows.isDebugRun) return false;
  const status = workflows.workflowRunDetail?.run.status;
  if (!status) return false;
  return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
});

function toggleMenu(menu: "nodes" | "arrange") {
  openMenu.value = openMenu.value === menu ? null : menu;
}

function closeMenu() {
  openMenu.value = null;
}

function addNode(kind: WorkflowNodeKind) {
  closeMenu();
  workflows.addWorkflowNode(kind);
}

function arrangeNodes(direction: WorkflowLayoutDirection) {
  closeMenu();
  workflows.autoArrangeWorkflowNodes(direction);
}

function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target;
  if (!(target instanceof Node)) return;
  if (toolbarRef.value?.contains(target)) return;
  closeMenu();
}

function onDocumentKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") closeMenu();
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocumentPointerDown);
  document.addEventListener("keydown", onDocumentKeyDown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  document.removeEventListener("keydown", onDocumentKeyDown);
});
</script>

<style scoped>
.workflow-actions {
  align-items: center;
}

.toolbar-menu {
  position: relative;
}

.toolbar-menu-panel {
  position: absolute;
  z-index: 30;
  top: calc(100% + 4px);
  left: 0;
  display: grid;
  min-width: 150px;
  padding: 4px;
  border: 1px solid #cbd5e1;
  border-radius: 6px;
  background: #ffffff;
  box-shadow: 0 10px 24px rgba(15, 23, 42, 0.16);
}

.toolbar-menu-panel button {
  justify-content: flex-start;
  border-color: transparent;
  background: transparent;
  color: #17202a;
  font-size: 12px;
}

.toolbar-menu-panel button:hover:not(:disabled) {
  background: #f1f5f9;
}

.run-debug-btn {
  background: #fef3c7;
  border-color: #f59e0b;
  color: #92400e;
  font-weight: 600;
}
.run-debug-btn:hover:not(:disabled) {
  background: #fde68a;
}
.stop-debug-btn {
  background: #dc2626;
  border-color: #dc2626;
  color: #fff;
  font-weight: 600;
}
.stop-debug-btn:hover:not(:disabled) {
  background: #b91c1c;
}
</style>
