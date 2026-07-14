<template>
  <section class="pane pipelines-pane">
    <SplitPane
      class="pipeline-layout"
      storage-key="command-center.pipelines.split"
      :initial-first-pct="72"
      :min-first="420"
      :min-second="280"
      collapsible-second
    >
      <template #first>
        <div class="panel pipeline-canvas-panel">
          <div class="panel-toolbar">
            <div class="pipeline-copy">
              <h2>Pipelines</h2>
              <p>Workflows linked by chaining triggers. Drag between workflows to create a chain.</p>
            </div>
            <button class="btn" :disabled="pipeline.loading" @click="pipeline.refresh">
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </div>
          <p v-if="pipeline.error" class="pipeline-error">{{ pipeline.error }}</p>
          <div class="pipeline-canvas-host">
            <PipelineCanvas @open-workflow="openWorkflow" />
          </div>
        </div>
      </template>

      <template #second>
        <div class="panel pipeline-inspector">
          <template v-if="selected">
            <h3>Chain</h3>
            <p class="pipeline-inspector-summary">
              <strong>{{ pipeline.nameById(selected.source) }}</strong>
              →
              <strong>{{ pipeline.nameById(selected.target) }}</strong>
            </p>
            <label class="pipeline-field">
              <span>Fires on</span>
              <select
                :value="selected.data.on"
                @change="onSelectorChange(($event.target as HTMLSelectElement).value)"
              >
                <option value="success">Success</option>
                <option value="failure">Failure</option>
                <option value="complete">Complete</option>
              </select>
            </label>
            <label class="pipeline-field pipeline-field-inline">
              <input
                type="checkbox"
                :checked="selected.data.enabled"
                @change="onEnabledChange(($event.target as HTMLInputElement).checked)"
              />
              <span>Enabled</span>
            </label>
            <button class="btn btn-danger" @click="pipeline.deleteSelected">
              <Icon name="trash" />
              <span>Delete chain</span>
            </button>
          </template>
          <p v-else class="pipeline-inspector-empty">
            Select a chain edge to edit it, or drag from one workflow to another to create one.
          </p>

          <div v-if="pipeline.unresolved.length" class="pipeline-unresolved">
            <h4>Unresolved chains</h4>
            <p class="hint">
              These chaining triggers point at a workflow name that no longer exists.
            </p>
            <ul>
              <li v-for="(item, index) in pipeline.unresolved" :key="index">
                <strong>{{ item.sourceName }}</strong> → “{{ item.targetName }}” (on {{ item.on }})
              </li>
            </ul>
          </div>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import { usePipelineStore } from "../adapters/pinia/pipeline";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import { useAppStore } from "../adapters/pinia/app";
import type { ChainEvent } from "../../core/workflow/pipeline-graph";
import SplitPane from "../components/shared/SplitPane.vue";
import Icon from "../components/shared/Icon.vue";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";

const pipeline = usePipelineStore();
const workflows = useWorkflowsStore();
const app = useAppStore();

const selected = computed(() => pipeline.selectedEdge);

function onSelectorChange(value: string) {
  void pipeline.updateSelected({ on: value as ChainEvent });
}

function onEnabledChange(enabled: boolean) {
  void pipeline.updateSelected({ enabled });
}

function openWorkflow(workflowId: string) {
  const workflow = workflows.workflows.find((wf) => wf.id === workflowId);

  if (workflow) {
    void workflows.selectWorkflow(workflow);
    app.activeTab = "Workflows";
  }
}

onMounted(() => {
  void pipeline.refresh();
});
</script>

<style scoped>
.pipelines-pane,
.pipeline-layout {
  width: 100%;
  height: 100%;
}

.pipeline-canvas-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.pipeline-canvas-host {
  flex: 1;
  min-height: 0;
}

.pipeline-copy h2 {
  margin: 0;
}

.pipeline-copy p {
  margin: 2px 0 0;
  color: var(--text-muted, #475467);
  font-size: 12px;
}

.pipeline-error {
  color: var(--danger, #b42318);
  padding: 6px 12px;
  margin: 0;
}

.pipeline-inspector {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}

.pipeline-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}

.pipeline-field-inline {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.pipeline-inspector-empty,
.pipeline-inspector .hint {
  color: var(--text-muted, #475467);
  font-size: 13px;
}

.pipeline-unresolved ul {
  margin: 6px 0 0;
  padding-left: 18px;
  font-size: 12px;
}
</style>
