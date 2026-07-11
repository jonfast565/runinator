<template>
  <div ref="toolbarRef" class="workflow-toolbar">
    <div class="workflow-title">
      <strong>
        {{ workflows.workflowDraft.name }}
        <span
          v-if="workflows.isDirty"
          class="unsaved-dot"
          title="Unsaved changes"
          aria-hidden="true"
        ></span>
      </strong>
      <span v-if="workflows.isDirty" class="unsaved-label">Unsaved changes</span>
      <span v-else
        >v{{ workflows.workflowDraft.version }} · concurrency
        {{ workflows.workflowConcurrency }}</span
      >
    </div>
    <div class="workflow-actions">
      <button class="btn" @click="workflows.openWorkflowSettings">
        <Icon name="settings" />
        <span>Settings</span>
      </button>
      <button v-if="workflows.selectedWorkflowId" class="btn" @click="shareOpen = true">
        <Icon name="approve" />
        <span>Share</span>
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
          <p v-if="catalogMetadata.loading || !catalogMetadata.loaded" class="catalog-loading-hint">
            <LoadingSpinner size="sm" label="Loading node types" />
            Loading node types…
          </p>
          <button
            v-for="kind in workflows.workflowNodeKinds"
            :key="kind"
            type="button"
            role="menuitem"
            class="btn btn-ghost node-menu-item"
            :title="workflowNodeKindDescription(kind)"
            :disabled="!catalogMetadata.loaded"
            @click="addNode(kind)"
          >
            <Icon
              :name="workflowNodeKindIcon(kind)"
              :size="14"
              class="node-menu-icon"
            />
            <span class="node-menu-text">
              <span class="node-menu-label">{{ workflowNodeKindLabel(kind) }}</span>
              <span class="node-menu-desc">{{ workflowNodeKindDescription(kind) }}</span>
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
          <button
            type="button"
            role="menuitem"
            class="btn btn-ghost"
            title="Arrange workflow nodes left to right"
            @click="arrangeNodes('horizontal')"
          >
            Left to right
          </button>
          <button
            type="button"
            role="menuitem"
            class="btn btn-ghost"
            title="Arrange workflow nodes top to bottom"
            @click="arrangeNodes('vertical')"
          >
            Top to bottom
          </button>
        </div>
      </div>
      <button
        class="btn"
        :class="{ 'btn-primary': workflows.isDirty }"
        :disabled="savingWorkflow"
        @click="workflows.saveSelectedWorkflow"
      >
        <LoadingSpinner v-if="savingWorkflow" size="sm" label="Saving workflow" />
        <Icon v-else name="save" />
        <span>{{ savingWorkflow ? "Saving…" : workflows.isDirty ? "Save changes" : "Save" }}</span>
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
          <button
            type="button"
            role="menuitem"
            class="btn btn-ghost"
            title="Export this workflow as a .wdl file"
            @click="exportWdl"
          >
            This workflow (.wdl)
          </button>
          <button
            type="button"
            role="menuitem"
            class="btn btn-ghost"
            title="Export all workflows as a .wdlp pack zip"
            @click="exportPack"
          >
            All workflows (.wdlp pack)
          </button>
        </div>
      </div>
      <button
        class="btn btn-primary"
        :disabled="!workflows.canRunWorkflow || startingRun"
        @click="workflows.runSelectedWorkflow()"
      >
        <LoadingSpinner v-if="startingRun" size="sm" label="Starting run" />
        <Icon v-else name="play" />
        <span>{{ startingRun ? "Starting…" : "Run" }}</span>
      </button>
      <button
        v-if="!isActiveDebugRun"
        class="btn btn-warn"
        :disabled="!workflows.canRunWorkflow || startingRun"
        @click="workflows.runSelectedWorkflowDebug"
      >
        <LoadingSpinner v-if="startingRun" size="sm" label="Starting debug run" />
        <Icon v-else name="debug" />
        <span>{{ startingRun ? "Starting…" : "Run Debug" }}</span>
      </button>
      <button
        v-else
        class="btn btn-danger"
        :disabled="!workflows.canCancelWorkflowRun || cancelingRun"
        title="Cancel the active debug run"
        @click="workflows.cancelSelectedWorkflowRun"
      >
        <LoadingSpinner v-if="cancelingRun" size="sm" label="Canceling run" />
        <Icon v-else name="stop" />
        <span>{{ cancelingRun ? "Stopping…" : "Stop Debug" }}</span>
      </button>
      <button
        class="btn"
        :disabled="!workflows.canRemoveSelectedStep"
        @click="workflows.removeWorkflowStep"
      >
        <Icon name="trash" />
        <span>Remove</span>
      </button>
    </div>
    <WorkflowSettingsModal v-if="workflows.workflowSettingsOpen" />
    <ShareWorkflowModal
      v-if="shareOpen && workflows.selectedWorkflowId"
      :workflow-id="workflows.selectedWorkflowId || ''"
      @close="shareOpen = false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { WorkflowLayoutDirection, WorkflowNodeKind } from "../../../core/domain/models";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../../ui/adapters/pinia/catalogMetadata";
import {
  workflowNodeKindDescription,
  workflowNodeKindIcon,
  workflowNodeKindLabel,
} from "../../../core/workflow";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import WorkflowSettingsModal from "./WorkflowSettingsModal.vue";
import ShareWorkflowModal from "./ShareWorkflowModal.vue";
import { useOperationLoading } from "../../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
const { isLoading: savingWorkflow } = useOperationLoading("Saving workflow");
const { isLoading: startingRun } = useOperationLoading("Running workflow", { prefix: true });
const { isLoading: cancelingRun } = useOperationLoading("Canceling workflow run", { prefix: true });
const toolbarRef = ref<HTMLElement | null>(null);
const openMenu = ref<"nodes" | "arrange" | "export" | null>(null);
const shareOpen = ref(false);

const isActiveDebugRun = computed(() => {
  if (!workflows.isDebugRun) {
    return false;
  }

  const status = workflows.workflowRunDetail?.run.status;

  if (!status) {
    return false;
  }

  return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
});

function toggleMenu(menu: "nodes" | "arrange" | "export") {
  if (menu === "nodes" && openMenu.value !== "nodes" && !catalogMetadata.loaded) {
    void catalogMetadata.fetchCatalogs();
  }

  openMenu.value = openMenu.value === menu ? null : menu;
}

function exportWdl() {
  closeMenu();
  void workflows.exportWorkflowWdl();
}

function exportPack() {
  closeMenu();
  void workflows.exportWorkflowPack();
}

function closeMenu() {
  openMenu.value = null;
}

function addNode(kind: WorkflowNodeKind) {
  if (!catalogMetadata.loaded) {
    return;
  }

  closeMenu();
  workflows.addWorkflowNode(kind);
}

function arrangeNodes(direction: WorkflowLayoutDirection) {
  closeMenu();
  workflows.autoArrangeWorkflowNodes(direction);
}

function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target;

  if (!(target instanceof Node)) {
    return;
  }

  if (toolbarRef.value?.contains(target)) {
    return;
  }

  closeMenu();
}

function onDocumentKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeMenu();
  }
}

// dropdown-dismissal listeners are attached only while a menu is open, not globally for the page.
watch(openMenu, (menu) => {
  if (menu) {
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("keydown", onDocumentKeyDown);
  } else {
    document.removeEventListener("pointerdown", onDocumentPointerDown);
    document.removeEventListener("keydown", onDocumentKeyDown);
  }
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

.unsaved-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  margin-left: 6px;
  border-radius: 50%;
  background: var(--warn-solid);
  vertical-align: middle;
}

.unsaved-label {
  color: var(--warning-fg);
  font-weight: 600;
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
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: var(--workflow-menu-shadow);
}

.toolbar-menu-panel button {
  justify-content: flex-start;
  border-color: transparent;
  background: transparent;
  color: var(--text);
  font-size: 12px;
}

.toolbar-menu-panel button:hover:not(:disabled) {
  background: var(--surface-hover);
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
  color: var(--accent-text);
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
  color: var(--text-muted);
  font-size: 10.5px;
  line-height: 1.35;
  white-space: normal;
}

.catalog-loading-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 10px 8px;
  color: var(--text-muted);
  font-size: 12px;
}
</style>
