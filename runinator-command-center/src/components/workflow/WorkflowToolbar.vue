<template>
  <div ref="toolbarRef" class="workflow-toolbar">
    <div class="workflow-title">
      <strong>{{ workflows.workflowDraft.name }}</strong>
      <span>v{{ workflows.workflowDraft.version }} · concurrency {{ workflows.workflowConcurrency }}</span>
    </div>
    <div class="workflow-actions">
      <button class="btn" @click="workflows.openWorkflowSettings">
        <Icon name="settings" />
        <span>Settings</span>
      </button>
      <div class="toolbar-menu">
        <button
          type="button"
          class="btn"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'nodes'"
          @click="toggleMenu('nodes')"
        >
          <Icon name="plus" />
          <span>New Node</span>
        </button>
        <div v-if="openMenu === 'nodes'" class="toolbar-menu-panel node-menu-panel" role="menu">
          <button
            v-for="kind in workflows.workflowNodeKinds"
            :key="kind"
            type="button"
            role="menuitem"
            class="btn btn-ghost node-menu-item"
            :title="workflowNodeKindInfo[kind]?.description"
            @click="addNode(kind)"
          >
            <Icon :name="workflowNodeKindInfo[kind]?.icon ?? 'box'" :size="14" class="node-menu-icon" />
            <span class="node-menu-text">
              <span class="node-menu-label">{{ kind }}</span>
              <span class="node-menu-desc">{{ workflowNodeKindInfo[kind]?.description }}</span>
            </span>
          </button>
        </div>
      </div>
      <div class="toolbar-menu">
        <button
          type="button"
          class="btn"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'arrange'"
          @click="toggleMenu('arrange')"
        >
          <Icon name="list" />
          <span>Arrange</span>
        </button>
        <div v-if="openMenu === 'arrange'" class="toolbar-menu-panel" role="menu">
          <button type="button" role="menuitem" class="btn btn-ghost" title="Arrange workflow nodes left to right" @click="arrangeNodes('horizontal')">
            Left to right
          </button>
          <button type="button" role="menuitem" class="btn btn-ghost" title="Arrange workflow nodes top to bottom" @click="arrangeNodes('vertical')">
            Top to bottom
          </button>
        </div>
      </div>
      <button class="btn" @click="workflows.saveSelectedWorkflow">
        <Icon name="save" />
        <span>Save</span>
      </button>
      <div class="toolbar-menu">
        <button
          type="button"
          class="btn"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'export'"
          @click="toggleMenu('export')"
        >
          <Icon name="save" />
          <span>Export</span>
        </button>
        <div v-if="openMenu === 'export'" class="toolbar-menu-panel" role="menu">
          <button type="button" role="menuitem" class="btn btn-ghost" title="Export this workflow as a .wdl file" @click="exportWdl">
            This workflow (.wdl)
          </button>
          <button type="button" role="menuitem" class="btn btn-ghost" title="Export all workflows as a .wdlp pack zip" @click="exportPack">
            All workflows (.wdlp pack)
          </button>
        </div>
      </div>
      <button class="btn btn-primary" :disabled="!workflows.canRunWorkflow" @click="workflows.runSelectedWorkflow()">
        <Icon name="play" />
        <span>Run</span>
      </button>
      <button
        v-if="!isActiveDebugRun"
        class="btn btn-warn"
        :disabled="!workflows.canRunWorkflow"
        @click="workflows.runSelectedWorkflowDebug"
      >
        <Icon name="debug" />
        <span>Run Debug</span>
      </button>
      <button
        v-else
        class="btn btn-danger"
        :disabled="!workflows.canCancelWorkflowRun"
        title="Cancel the active debug run"
        @click="workflows.cancelSelectedWorkflowRun"
      >
        <Icon name="stop" />
        <span>Stop Debug</span>
      </button>
      <button class="btn" :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">
        <Icon name="trash" />
        <span>Remove</span>
      </button>
    </div>
    <WorkflowSettingsModal v-if="workflows.workflowSettingsOpen" />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { WorkflowLayoutDirection, WorkflowNodeKind } from "../../types/models";
import { useWorkflowsStore } from "../../stores/workflows";
import { workflowNodeKindInfo } from "../../utils/workflows";
import Icon from "../shared/Icon.vue";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";

const workflows = useWorkflowsStore();
const toolbarRef = ref<HTMLElement | null>(null);
const openMenu = ref<"nodes" | "arrange" | "export" | null>(null);

const isActiveDebugRun = computed(() => {
  if (!workflows.isDebugRun) return false;
  const status = workflows.workflowRunDetail?.run.status;
  if (!status) return false;
  return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
});

function toggleMenu(menu: "nodes" | "arrange" | "export") {
  openMenu.value = openMenu.value === menu ? null : menu;
}

function exportWdl() {
  closeMenu();
  workflows.exportWorkflowWdl();
}

function exportPack() {
  closeMenu();
  workflows.exportWorkflowPack();
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

.node-menu-panel {
  min-width: 240px;
  max-height: 60vh;
  overflow-y: auto;
}

.node-menu-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 8px;
  text-align: left;
}

.node-menu-icon {
  margin-top: 2px;
  color: #3498db;
}

.node-menu-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.node-menu-label {
  font-weight: 600;
  text-transform: capitalize;
}

.node-menu-desc {
  color: #64748b;
  font-size: 10.5px;
  line-height: 1.35;
  white-space: normal;
}

</style>
