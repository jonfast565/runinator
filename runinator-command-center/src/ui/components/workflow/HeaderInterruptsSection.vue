<template>
  <section class="detail-section interrupt-manager">
    <div class="interrupt-manager-heading">
      <div>
        <div class="flex items-center gap-1">
          <h3>Interrupt handlers</h3>
          <HelpBubble label="About interrupt handler routes">
            When the chosen event happens, Runinator pauses the affected thread, runs this handler's
            steps, then follows its <code>resume</code> decision. The main workflow does not
            continue until the handler finishes.
          </HelpBubble>
        </div>
        <p class="section-note">
          Choose an event, create its safe route, then add the steps that should handle it.
        </p>
      </div>
    </div>

    <div class="interrupt-overview" aria-label="Interrupt handler summary">
      <button type="button" :class="{ active: filter === 'all' }" @click="filter = 'all'">
        <strong>{{ declarations.length }}</strong>
        <span>All</span>
      </button>
      <button type="button" :class="{ active: filter === 'enabled' }" @click="filter = 'enabled'">
        <strong>{{ enabledCount }}</strong>
        <span>Enabled</span>
      </button>
      <button
        type="button"
        :class="[{ active: filter === 'attention' }, { 'has-issues': issueCount > 0 }]"
        @click="filter = 'attention'"
      >
        <strong>{{ issueCount }}</strong>
        <span>Issues</span>
      </button>
    </div>

    <div class="interrupt-create-row">
      <label>
        <span>When should the handler run?</span>
        <select v-model="newSource" :disabled="undeclared.length === 0">
          <option v-for="source in undeclared" :key="source" :value="source">
            {{ labelFor(source) }}
          </option>
        </select>
      </label>
      <div v-if="newSource" class="interrupt-source-preview" aria-live="polite">
        <strong>{{ labelFor(newSource) }}</strong>
        <span>{{ describeSource(newSource) }}</span>
      </div>
      <div class="interrupt-create-actions">
        <button
          type="button"
          class="btn btn-primary btn-sm"
          :disabled="!newSource || !catalogMetadata.loaded"
          title="Create a complete interrupt-to-resume route and show it on the canvas"
          @click="scaffold"
        >
          <Icon name="plus" :size="14" />
          Create handler route
        </button>
        <span v-if="undeclared.length === 0" class="hint">All sources configured</span>
      </div>
    </div>

    <div v-if="declarations.length === 0" class="interrupt-empty-state">
      <span class="interrupt-empty-icon"><Icon name="bolt" :size="19" /></span>
      <div>
        <strong>No interrupt handlers</strong>
        <span>Add a source above to create an interrupt → resume route on the canvas.</span>
      </div>
    </div>

    <div v-else class="interrupt-list-tools">
      <label class="interrupt-search">
        <Icon name="search" :size="14" />
        <input v-model.trim="query" type="search" placeholder="Find a handler…" />
      </label>
      <span>{{ filteredDeclarations.length }} shown</span>
    </div>

    <div v-if="unassignedIssues.length" class="interrupt-unassigned-issues">
      <Icon name="alert" :size="14" />
      <div>
        <strong>Workflow-level interrupt issues</strong>
        <button
          v-for="issue in unassignedIssues"
          :key="`${issue.severity}-${issue.nodeId}-${issue.message}`"
          type="button"
          @click="focusIssue('', issue.nodeId)"
        >
          {{ issue.message }}
        </button>
      </div>
    </div>

    <div v-if="filteredDeclarations.length" class="interrupt-handler-list">
      <article
        v-for="item in filteredDeclarations"
        :key="item.entry.handler"
        class="interrupt-handler-card"
        :class="[`is-${handlerState(item.entry)}`, { 'is-disabled': !item.entry.enabled }]"
      >
        <header class="interrupt-handler-card-header">
          <div class="interrupt-handler-card-title">
            <span class="interrupt-handler-icon"><Icon name="bolt" :size="15" /></span>
            <div>
              <span class="interrupt-handler-source">{{ labelFor(item.entry.source) }}</span>
              <code>{{ item.entry.handler }}</code>
            </div>
          </div>
          <span class="interrupt-handler-state">
            <span aria-hidden="true"></span>{{ handlerStateLabel(item.entry) }}
          </span>
        </header>

        <div class="interrupt-handler-route" aria-label="Handler route">
          <span class="interrupt-route-node">Event</span>
          <span class="interrupt-route-line" aria-hidden="true"></span>
          <strong>{{ routeStepLabel(item.entry.handler) }}</strong>
          <span class="interrupt-route-line" aria-hidden="true"></span>
          <span class="interrupt-route-node">Resume</span>
        </div>
        <p class="interrupt-handler-description">{{ describeSource(item.entry.source) }}</p>

        <div v-if="issuesFor(item.entry.handler).length" class="interrupt-handler-issues">
          <button
            v-for="issue in issuesFor(item.entry.handler)"
            :key="`${issue.severity}-${issue.nodeId}-${issue.message}`"
            type="button"
            :class="`issue-${issue.severity}`"
            @click="focusIssue(item.entry.handler, issue.nodeId)"
          >
            <Icon name="alert" :size="13" />
            <span>{{ issue.message }}</span>
          </button>
        </div>

        <div class="interrupt-handler-primary-actions">
          <button
            type="button"
            class="btn btn-sm btn-primary"
            @click="focusHandler(item.entry.handler)"
          >
            {{ stepCount(item.entry.handler) === 0 ? "Add first step" : "Show on canvas" }}
          </button>
          <button type="button" class="btn btn-sm" @click="editEntry(item.entry.handler)">
            Edit first step
          </button>
          <button
            type="button"
            class="btn btn-sm interrupt-configure-button"
            :aria-expanded="configurationOpen === item.entry.handler"
            :aria-controls="`interrupt-config-${item.entry.handler}`"
            @click="toggleConfiguration(item.entry.handler)"
          >
            <Icon name="settings" :size="13" />
            Settings
          </button>
        </div>

        <div
          v-if="configurationOpen === item.entry.handler"
          :id="`interrupt-config-${item.entry.handler}`"
          class="interrupt-handler-config"
        >
          <label class="interrupt-source-field">
            Interrupt type
            <select
              :value="item.entry.source"
              @change="workflows.setHeaderInterruptSource(item.index, selectValue($event))"
            >
              <option
                v-for="option in availableSources(item.entry.source)"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </label>
          <label v-if="item.entry.source === 'timer'" class="interrupt-source-field">
            Repeat every (seconds)
            <input
              type="number"
              min="1"
              step="1"
              :value="item.entry.intervalSeconds ?? 60"
              @change="workflows.setHeaderInterruptInterval(item.index, numberValue($event))"
            />
          </label>
          <label class="interrupt-enabled-toggle checkbox">
            <input
              type="checkbox"
              :checked="item.entry.enabled"
              @change="workflows.setHeaderInterruptEnabled(item.index, checkedValue($event))"
            />
            Handler enabled
          </label>
          <button
            type="button"
            class="btn btn-sm btn-danger"
            @click="workflows.removeHeaderInterrupt(item.index)"
          >
            <Icon name="trash" :size="13" />
            Delete handler and route
          </button>
        </div>
      </article>
    </div>

    <div v-else-if="declarations.length" class="interrupt-no-results">
      <Icon name="search" :size="18" />
      <strong>No matching handlers</strong>
      <span>Try another search or show all handler states.</span>
      <button type="button" class="btn btn-sm" @click="clearFilters">Clear filters</button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { WorkflowValidationIssue } from "../../../core/domain/models";
import type { InterruptDeclaration } from "../../../core/workflow/interrupt-regions";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import Icon from "../shared/Icon.vue";
import HelpBubble from "../shared/HelpBubble.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();

const declarations = computed(() => workflows.headerDraft.interrupts);
const sourceOptions = computed(() => catalogMetadata.enumOptions("interrupt_source"));
const undeclared = computed(() =>
  workflows.getUndeclaredInterruptSources(sourceOptions.value.map((option) => option.value)),
);
const newSource = ref("");
const query = ref("");
const filter = ref<"all" | "enabled" | "attention">("all");
const configurationOpen = ref<string | null>(null);

const allIssues = computed(() => {
  void workflows.headerDraft;
  void workflows.workflowLayoutVersion;
  void workflows.workflowJson;
  return workflows.getInterruptIssues();
});
const issueCount = computed(() => allIssues.value.length);
const enabledCount = computed(() => declarations.value.filter((entry) => entry.enabled).length);
const unassignedIssues = computed(() =>
  allIssues.value.filter((issue) => !issue.interruptHandlerId),
);
const filteredDeclarations = computed(() => {
  const needle = query.value.toLocaleLowerCase();

  return declarations.value
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => {
      if (filter.value === "enabled" && !entry.enabled) {
        return false;
      }

      if (filter.value === "attention" && issuesFor(entry.handler).length === 0) {
        return false;
      }

      return (
        !needle ||
        entry.handler.toLocaleLowerCase().includes(needle) ||
        entry.source.toLocaleLowerCase().includes(needle) ||
        labelFor(entry.source).toLocaleLowerCase().includes(needle)
      );
    });
});

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
  return allIssues.value.filter((issue) => issue.interruptHandlerId === handler);
}

function handlerState(entry: InterruptDeclaration): "ready" | "disabled" | "attention" {
  if (issuesFor(entry.handler).length > 0) {
    return "attention";
  }

  return entry.enabled ? "ready" : "disabled";
}

function handlerStateLabel(entry: InterruptDeclaration): string {
  const state = handlerState(entry);
  return state === "attention" ? "Needs attention" : state === "disabled" ? "Disabled" : "Ready";
}

function routeStepLabel(handler: string): string {
  const count = stepCount(handler);
  return count === 0 ? "Empty route" : `${String(count)} step${count === 1 ? "" : "s"}`;
}

function toggleConfiguration(handler: string) {
  configurationOpen.value = configurationOpen.value === handler ? null : handler;
}

function clearFilters() {
  query.value = "";
  filter.value = "all";
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
