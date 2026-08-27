<template>
  <section class="detail-section interrupt-manager">
    <div class="interrupt-manager-heading">
      <div>
        <h3>
          Handler routes <span class="count-pill">{{ declarations.length }}</span>
        </h3>
        <p class="section-note">
          Each route suspends a thread, handles the interrupt on the canvas, then returns at
          <code>resume</code>. Add steps directly on its highlighted connections.
        </p>
      </div>
      <div class="interrupt-add-control">
        <label>
          New source
          <select v-model="newSource" :disabled="undeclared.length === 0">
            <option v-for="source in undeclared" :key="source" :value="source">
              {{ labelFor(source) }}
            </option>
          </select>
        </label>
        <button
          type="button"
          class="btn btn-primary btn-sm"
          :disabled="!newSource || !catalogMetadata.loaded"
          title="Create an interrupt-to-resume route and focus it on the canvas"
          @click="scaffold"
        >
          <Icon name="plus" :size="14" />
          Add handler
        </button>
      </div>
    </div>

    <div v-if="declarations.length === 0" class="interrupt-empty-state">
      <Icon name="bolt" :size="19" />
      <div>
        <strong>No handler routes yet</strong>
        <span>Choose a source to create an empty route, then insert its first canvas step.</span>
      </div>
    </div>

    <div v-else class="interrupt-handler-list">
      <article
        v-for="(entry, index) in declarations"
        :key="entry.handler"
        class="interrupt-handler-card"
        :class="[`is-${handlerState(entry)}`, { 'is-disabled': !entry.enabled }]"
      >
        <header class="interrupt-handler-card-header">
          <div class="interrupt-handler-card-title">
            <Icon name="bolt" :size="15" />
            <div>
              <span class="interrupt-handler-source">{{ labelFor(entry.source) }}</span>
              <code>{{ entry.handler }}</code>
            </div>
          </div>
          <span class="interrupt-handler-state">{{ handlerStateLabel(entry) }}</span>
        </header>

        <p class="interrupt-handler-route">
          <span>interrupt</span><span aria-hidden="true">→</span>
          <strong>{{
            stepCount(entry.handler) === 0
              ? "add first step"
              : `${stepCount(entry.handler)} step${stepCount(entry.handler) === 1 ? "" : "s"}`
          }}</strong>
          <span aria-hidden="true">→</span><span>resume</span>
        </p>
        <p class="hint">{{ describeSource(entry.source) }}</p>

        <div v-if="issuesFor(entry.handler).length" class="interrupt-handler-issues">
          <button
            v-for="issue in issuesFor(entry.handler)"
            :key="`${issue.severity}-${issue.nodeId}-${issue.message}`"
            type="button"
            :class="`issue-${issue.severity}`"
            @click="focusIssue(entry.handler, issue.nodeId)"
          >
            <Icon name="alert" :size="13" />
            <span>{{ issue.message }}</span>
          </button>
        </div>

        <div class="interrupt-handler-actions">
          <button type="button" class="btn btn-sm" @click="focusHandler(entry.handler)">
            {{ stepCount(entry.handler) === 0 ? "Add first step" : "Focus route" }}
          </button>
          <button type="button" class="btn btn-sm" @click="editEntry(entry.handler)">
            Edit entry
          </button>
          <label class="interrupt-source-field">
            Source
            <select
              :value="entry.source"
              @change="workflows.setHeaderInterruptSource(index, selectValue($event))"
            >
              <option
                v-for="option in availableSources(entry.source)"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </label>
          <label v-if="entry.source === 'timer'" class="interrupt-source-field">
            Every (seconds)
            <input
              type="number"
              min="1"
              step="1"
              :value="entry.intervalSeconds ?? 60"
              @change="workflows.setHeaderInterruptInterval(index, numberValue($event))"
            />
          </label>
          <label class="interrupt-enabled-toggle checkbox">
            <input
              type="checkbox"
              :checked="entry.enabled"
              @change="workflows.setHeaderInterruptEnabled(index, checkedValue($event))"
            />
            Enabled
          </label>
          <button
            type="button"
            class="btn btn-sm btn-danger"
            @click="workflows.removeHeaderInterrupt(index)"
          >
            Delete
          </button>
        </div>
      </article>
    </div>

    <p v-if="undeclared.length === 0" class="hint">
      Every available interrupt source has a handler.
    </p>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { WorkflowValidationIssue } from "../../../core/domain/models";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import Icon from "../shared/Icon.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

const declarations = computed(() => workflows.headerDraft.interrupts);
const sourceOptions = computed(() => catalogMetadata.enumOptions("interrupt_source"));
const undeclared = computed(() =>
  workflows.getUndeclaredInterruptSources(sourceOptions.value.map((option) => option.value)),
);
const newSource = ref("");

watch(
  undeclared,
  (sources) => {
    if (!sources.includes(newSource.value)) {
      newSource.value = sources.at(0) ?? "";
    }
  },
  { immediate: true },
);

function selectValue(event: Event): string {
  return (event.target as HTMLSelectElement).value;
}

function checkedValue(event: Event): boolean {
  return (event.target as HTMLInputElement).checked;
}

function numberValue(event: Event): number {
  return Number((event.target as HTMLInputElement).value);
}

function labelFor(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.label ?? source;
}

function describeSource(source: string): string {
  return sourceOptions.value.find((option) => option.value === source)?.description ?? "";
}

function availableSources(current: string) {
  const allowed = new Set([current, "timer", ...undeclared.value]);
  return sourceOptions.value.filter((option) => allowed.has(option.value));
}

function regionNodeIds(handler: string): string[] {
  return workflows.getRegionNodeIds(handler);
}

function stepCount(handler: string): number {
  return Math.max(0, regionNodeIds(handler).length - 2);
}

function issuesFor(handler: string): WorkflowValidationIssue[] {
  return workflows.getInterruptIssues().filter((issue) => issue.interruptHandlerId === handler);
}

function handlerState(entry: {
  handler: string;
  enabled: boolean;
}): "ready" | "disabled" | "attention" {
  if (issuesFor(entry.handler).length > 0) {
    return "attention";
  }

  return entry.enabled ? "ready" : "disabled";
}

function handlerStateLabel(entry: { handler: string; enabled: boolean }): string {
  const state = handlerState(entry);
  return state === "attention" ? "Needs attention" : state === "disabled" ? "Disabled" : "Ready";
}

function focusHandler(handler: string) {
  workflows.focusWorkflowCanvasNodes(regionNodeIds(handler));
}

function focusIssue(handler: string, nodeId: string) {
  workflows.focusWorkflowCanvasNodes(nodeId === "workflow" ? regionNodeIds(handler) : [nodeId]);
}

function editEntry(nodeId: string) {
  workflows.openStepEditor(nodeId);
}

function scaffold() {
  workflows.scaffoldInterruptHandler(newSource.value);
}
</script>
