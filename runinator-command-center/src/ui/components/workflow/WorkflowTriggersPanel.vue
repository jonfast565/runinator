<template>
  <section class="form-section workflow-settings-section trigger-section">
    <template v-if="!workflows.triggerEditorOpen">
      <div class="workflow-settings-section-heading trigger-section-heading">
        <div>
          <div class="trigger-heading-line">
            <div>
              <p class="workflow-settings-eyebrow">Automation</p>
              <h3>Triggers</h3>
            </div>
            <span v-if="workflows.workflowTriggers.length" class="trigger-count">
              {{ workflows.workflowTriggers.length }}
            </span>
          </div>
          <p class="hint">Choose how and when this workflow starts.</p>
        </div>
        <div class="trigger-heading-actions">
          <button
            type="button"
            class="btn btn-icon"
            title="Refresh triggers"
            aria-label="Refresh triggers"
            :disabled="!workflows.canManageWorkflowTriggers"
            @click="workflows.refreshWorkflowTriggers"
          >
            <Icon name="refresh" :size="14" />
          </button>
          <button
            type="button"
            class="btn btn-primary"
            :disabled="!workflows.canManageWorkflowTriggers || !catalogMetadata.loaded"
            @click="triggerPickerOpen = !triggerPickerOpen"
          >
            <Icon name="plus" :size="14" />
            New trigger
          </button>
        </div>
      </div>

      <div v-if="triggerPickerOpen" class="trigger-picker" aria-labelledby="trigger-picker-title">
        <div class="trigger-picker-heading">
          <div>
            <h4 id="trigger-picker-title">How should this workflow start?</h4>
            <p>Pick a starting point. You can fine-tune it before saving.</p>
          </div>
          <button
            type="button"
            class="btn-close"
            aria-label="Close trigger type chooser"
            @click="triggerPickerOpen = false"
          >
            <Icon name="close" :size="14" />
          </button>
        </div>
        <div class="trigger-kind-grid">
          <button
            v-for="kind in catalogMetadata.triggerKinds"
            :key="kind.kind"
            type="button"
            class="trigger-kind-option"
            @click="startTrigger(kind.kind)"
          >
            <span class="trigger-kind-icon"
              ><Icon :name="triggerIcon(kind.kind)" :size="18"
            /></span>
            <span class="trigger-kind-copy">
              <strong>{{ kind.label }}</strong>
              <small>{{ kind.description || kindDescription(kind.kind) }}</small>
            </span>
            <Icon name="chevron-right" :size="15" />
          </button>
        </div>
      </div>

      <p v-if="!workflows.canManageWorkflowTriggers" class="hint trigger-callout">
        Save this workflow before adding automation.
      </p>
      <p v-else-if="!catalogMetadata.loaded" class="hint catalog-loading-hint trigger-callout">
        <LoadingSpinner size="sm" label="Loading trigger types" />
        Loading trigger types…
      </p>

      <div
        v-else-if="workflows.workflowTriggers.length === 0 && !triggerPickerOpen"
        class="trigger-empty-state"
      >
        <span class="trigger-empty-icon"><Icon name="bolt" :size="20" /></span>
        <div>
          <h4>Start this workflow automatically</h4>
          <p>Add a schedule, a manual start, an upstream workflow, or another supported trigger.</p>
        </div>
        <button type="button" class="btn" @click="triggerPickerOpen = true">
          Choose a trigger
        </button>
      </div>

      <div v-else-if="workflows.workflowTriggers.length" class="trigger-card-list">
        <article
          v-for="trigger in workflows.workflowTriggers"
          :key="trigger.id ?? `${trigger.kind}-${trigger.workflow_id}`"
          class="trigger-card"
          :class="{ 'is-disabled': !trigger.enabled }"
        >
          <span class="trigger-card-icon">
            <Icon :name="triggerIcon(trigger.kind)" :size="17" />
          </span>
          <div class="trigger-card-content">
            <div class="trigger-card-title">
              <strong>{{ triggerKindLabel(trigger.kind) }}</strong>
              <span
                class="workflow-state-pill"
                :class="trigger.enabled ? 'is-enabled' : 'is-disabled'"
              >
                {{ trigger.enabled ? "Active" : "Paused" }}
              </span>
            </div>
            <p class="trigger-card-summary">{{ triggerSummary(trigger) }}</p>
            <div class="trigger-card-meta">
              <span v-if="trigger.kind === 'cron'">
                <Icon name="clock" :size="12" />
                {{
                  trigger.enabled
                    ? nextRunLabel(trigger.next_execution)
                    : "Will not run while paused"
                }}
              </span>
              <span v-else>{{
                triggerKindMeta(trigger.kind)?.description || kindDescription(trigger.kind)
              }}</span>
            </div>
          </div>
          <div class="trigger-card-actions">
            <button type="button" class="btn" @click="workflows.editWorkflowTrigger(trigger)">
              <Icon name="edit" :size="13" />
              Edit
            </button>
            <button
              type="button"
              class="btn btn-icon btn-danger"
              title="Delete trigger"
              aria-label="Delete trigger"
              @click="workflows.deleteSelectedWorkflowTrigger(trigger)"
            >
              <Icon name="trash" :size="13" />
            </button>
          </div>
        </article>
      </div>
    </template>

    <section
      v-else
      ref="editor"
      class="trigger-editor"
      aria-labelledby="trigger-editor-title"
      tabindex="-1"
    >
      <header class="trigger-editor-header">
        <button type="button" class="trigger-back-button" @click="workflows.closeTriggerEditor">
          <Icon name="chevron-left" :size="15" />
          All triggers
        </button>
        <div class="trigger-editor-heading">
          <span class="trigger-editor-icon">
            <Icon :name="triggerIcon(workflows.triggerDraft.kind)" :size="19" />
          </span>
          <div>
            <p class="workflow-settings-eyebrow">
              {{ workflows.triggerEditorCreating ? "New trigger" : "Edit trigger" }}
            </p>
            <h3 id="trigger-editor-title">
              {{ triggerKindLabel(workflows.triggerDraft.kind) }} trigger
            </h3>
            <p>{{ currentKindDescription }}</p>
          </div>
        </div>
      </header>

      <div class="trigger-editor-topline">
        <label class="trigger-type-field">
          <span>Trigger type</span>
          <select
            v-model="workflows.triggerDraft.kind"
            :disabled="!catalogMetadata.loaded"
            @change="workflows.setTriggerKind(workflows.triggerDraft.kind)"
          >
            <option v-if="!catalogMetadata.loaded" value="" disabled>Loading trigger types…</option>
            <option
              v-for="kind in catalogMetadata.triggerKinds"
              :key="kind.kind"
              :value="kind.kind"
            >
              {{ kind.label }}
            </option>
          </select>
          <small v-if="!workflows.triggerEditorCreating"
            >Changing the type keeps compatible configuration values.</small
          >
        </label>
        <label class="trigger-status-control">
          <span class="trigger-status-copy">
            <strong>{{
              workflows.triggerDraft.enabled ? "Trigger active" : "Trigger paused"
            }}</strong>
            <small>{{
              workflows.triggerDraft.enabled
                ? "New runs can start from this trigger."
                : "Save it now and activate it when ready."
            }}</small>
          </span>
          <span class="switch" :class="{ 'is-on': workflows.triggerDraft.enabled }">
            <input v-model="workflows.triggerDraft.enabled" type="checkbox" />
            <span aria-hidden="true"></span>
          </span>
        </label>
      </div>

      <div v-if="workflows.triggerDraft.kind === 'cron'" class="trigger-editor-section">
        <div class="trigger-step-heading">
          <span>1</span>
          <div>
            <h4>Choose a schedule</h4>
            <p>Use a simple recurrence or enter a precise cron or RRULE schedule.</p>
          </div>
        </div>
        <ScheduleEditor
          :model-value="triggerSchedule"
          title="Run schedule"
          description="All previews use the selected timezone."
          @update:model-value="setTriggerSchedule"
        />
      </div>

      <div v-if="triggerConfigFields.length" class="trigger-editor-section">
        <div class="trigger-step-heading">
          <span>{{ workflows.triggerDraft.kind === "cron" ? 2 : 1 }}</span>
          <div>
            <h4>Configure the trigger</h4>
            <p>Provide the details this trigger needs to start the workflow.</p>
          </div>
        </div>
        <div class="trigger-field-list trigger-fields-card">
          <div
            v-for="field in triggerConfigFields"
            :key="field.name"
            class="trigger-catalog-field"
            :class="{ 'has-error': triggerValidation.errors.fields[field.name] }"
          >
            <CatalogFieldEditor
              :field="toNodeField(field)"
              :model-value="configDraft[field.name]"
              :workflows="workflows.workflows"
              @update:model-value="setConfigField(field.name, $event)"
            />
            <small v-if="triggerValidation.errors.fields[field.name]" class="field-error">
              {{ triggerValidation.errors.fields[field.name] }}
            </small>
          </div>
        </div>
      </div>

      <div
        v-if="workflows.triggerDraft.kind === 'cron'"
        class="trigger-editor-section trigger-blackout-section"
      >
        <div class="trigger-step-heading">
          <span>{{ triggerConfigFields.length ? 3 : 2 }}</span>
          <div>
            <h4>Protect quiet periods <em>Optional</em></h4>
            <p>Skip occurrences during maintenance, holidays, or another recurring window.</p>
          </div>
        </div>
        <label class="trigger-blackout-toggle">
          <input :checked="blackoutEnabled" type="checkbox" @change="toggleBlackout" />
          <span>Add a recurring blackout</span>
        </label>
        <ScheduleEditor
          v-if="blackoutEnabled"
          :model-value="triggerBlackout"
          window
          title="Blackout schedule"
          description="Occurrences inside this window are skipped and recorded as excluded."
          @update:model-value="setTriggerBlackout"
        />
      </div>

      <div
        v-if="workflows.triggerDraft.kind !== 'cron' && !triggerConfigFields.length"
        class="trigger-ready-state"
      >
        <span><Icon name="check" :size="17" /></span>
        <div>
          <strong>No additional setup needed</strong>
          <p>This trigger is ready to save. Advanced values remain available below.</p>
        </div>
      </div>

      <details class="trigger-advanced">
        <summary>
          <span>
            <Icon name="settings" :size="14" />
            Advanced settings
          </span>
          <small>Execution override, raw configuration, and metadata</small>
        </summary>
        <div class="trigger-advanced-content">
          <div class="trigger-window">
            <div class="trigger-window-heading">
              <div>
                <h4>Next execution override</h4>
                <p>Leave blank to let the scheduler calculate the next occurrence.</p>
              </div>
              <span>Local time</span>
            </div>
            <label :class="{ 'has-error': triggerValidation.errors.nextExecution }">
              <span>Run next at</span>
              <input
                v-model="workflows.triggerDraft.next_execution"
                type="datetime-local"
                :aria-invalid="Boolean(triggerValidation.errors.nextExecution)"
                aria-describedby="trigger-next-error"
              />
              <small
                v-if="triggerValidation.errors.nextExecution"
                id="trigger-next-error"
                class="field-error"
              >
                {{ triggerValidation.errors.nextExecution }}
              </small>
            </label>
          </div>
          <div class="trigger-json-grid">
            <div
              class="form-field"
              :class="{ 'has-error': triggerValidation.errors.configuration }"
            >
              <span class="form-field-label">Raw configuration</span>
              <small>Use this for fields that are not exposed by the guided editor.</small>
              <JsonEditor v-model="workflows.triggerJson.configuration" />
              <small v-if="triggerValidation.errors.configuration" class="field-error">
                {{ triggerValidation.errors.configuration }}
              </small>
            </div>
            <div class="form-field" :class="{ 'has-error': triggerValidation.errors.metadata }">
              <span class="form-field-label">Metadata</span>
              <small>Optional machine-readable context stored with this trigger.</small>
              <JsonEditor v-model="workflows.triggerJson.metadata" />
              <small v-if="triggerValidation.errors.metadata" class="field-error">
                {{ triggerValidation.errors.metadata }}
              </small>
            </div>
          </div>
        </div>
      </details>

      <p v-if="workflows.triggerEditorError" class="error m-0 text-xs" role="alert">
        {{ workflows.triggerEditorError }}
      </p>
      <footer class="trigger-editor-actions">
        <button type="button" class="btn" @click="workflows.closeTriggerEditor">Cancel</button>
        <button
          type="button"
          class="btn btn-primary"
          :disabled="Boolean(triggerValidation.error)"
          @click="workflows.submitWorkflowTrigger"
        >
          <Icon name="save" :size="14" />
          {{ workflows.triggerEditorCreating ? "Create trigger" : "Save changes" }}
        </button>
      </footer>
    </section>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type {
  NodeFieldMetadata,
  ScheduleSpec,
  UiField,
  WorkflowTrigger,
  WorkflowTriggerKind,
  WorkflowTriggerKindMetadata,
} from "../../../core/domain/models";
import type { IconName } from "../../../core/domain/icons";
import { formatDate } from "../../../core/utils/format";
import { defaultSchedule } from "../../../core/workflow/schedule";
import { validateTriggerEditor } from "../../../core/workflow/trigger-validation";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";
import Icon from "../shared/Icon.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import ScheduleEditor from "../shared/ScheduleEditor.vue";

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
const triggerPickerOpen = ref(false);
const editor = ref<HTMLElement | null>(null);

const currentKindMeta = computed(() => triggerKindMeta(workflows.triggerDraft.kind));
const currentKindDescription = computed(
  () => currentKindMeta.value?.description ?? kindDescription(workflows.triggerDraft.kind),
);
const triggerValidation = computed(() =>
  validateTriggerEditor(
    workflows.triggerDraft,
    workflows.triggerJson.configuration,
    workflows.triggerJson.metadata,
    currentKindMeta.value,
  ),
);
const configDraft = computed<Record<string, unknown>>(() => {
  try {
    const parsed = JSON.parse(workflows.triggerJson.configuration) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
});
const triggerConfigFields = computed(() =>
  (currentKindMeta.value?.fields ?? []).filter(
    (field) =>
      workflows.triggerDraft.kind !== "cron" ||
      !["cron", "schedule", "exclusions"].includes(field.name),
  ),
);
const triggerSchedule = computed<ScheduleSpec>(() => {
  const schedule = configDraft.value.schedule as ScheduleSpec | undefined;

  if (schedule?.recurrence) {
    return schedule;
  }

  const cron = typeof configDraft.value.cron === "string" ? configDraft.value.cron : "0 9 * * 1-5";
  return { recurrence: { kind: "cron", expression: cron }, timezone: "UTC", duration_seconds: 0 };
});
const blackoutEnabled = computed(
  () => Array.isArray(configDraft.value.exclusions) && configDraft.value.exclusions.length > 0,
);
const triggerBlackout = computed<ScheduleSpec>(() => {
  const exclusions = configDraft.value.exclusions;
  return Array.isArray(exclusions) && exclusions[0] && typeof exclusions[0] === "object"
    ? (exclusions[0] as ScheduleSpec)
    : defaultSchedule(true);
});

function triggerKindMeta(kind: WorkflowTriggerKind): WorkflowTriggerKindMetadata | undefined {
  return catalogMetadata.triggerKind(kind);
}

function triggerKindLabel(kind: WorkflowTriggerKind): string {
  return triggerKindMeta(kind)?.label ?? kind;
}

function kindIcon(kind: WorkflowTriggerKind): IconName {
  return kind === "cron" ? "calendar" : kind === "chained" ? "link" : "play";
}

function triggerIcon(kind: WorkflowTriggerKind): IconName {
  return (triggerKindMeta(kind)?.icon ?? kindIcon(kind)) as IconName;
}

function kindDescription(kind: WorkflowTriggerKind): string {
  if (kind === "cron") {
    return "Starts this workflow on a recurring schedule.";
  }

  if (kind === "chained") {
    return "Starts after an upstream workflow reaches the configured state.";
  }

  return "Allows this workflow to be started deliberately on demand.";
}

function startTrigger(kind: WorkflowTriggerKind) {
  triggerPickerOpen.value = false;
  workflows.addWorkflowTrigger(kind);
}

function triggerSummary(trigger: WorkflowTrigger): string {
  if (trigger.kind === "cron") {
    return workflows.triggerCronSummary(trigger) || "Schedule not configured";
  }

  if (trigger.kind === "chained") {
    return "Starts from an upstream workflow";
  }

  return "Available for on-demand starts";
}

function nextRunLabel(value: string | null | undefined): string {
  return value ? `Next run ${formatDate(value)}` : "Next run is being calculated";
}

function writeConfig(next: Record<string, unknown>) {
  workflows.triggerJson.configuration = JSON.stringify(next, null, 2);
}

function setTriggerSchedule(schedule: ScheduleSpec) {
  const next: Record<string, unknown> = { ...configDraft.value, schedule };

  if (schedule.recurrence.kind === "cron") {
    next.cron = schedule.recurrence.expression;
  }

  writeConfig(next);
}

function setTriggerBlackout(schedule: ScheduleSpec) {
  writeConfig({ ...configDraft.value, exclusions: [schedule] });
}

function toggleBlackout(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  const next = { ...configDraft.value };

  if (enabled) {
    next.exclusions = [defaultSchedule(true)];
  } else {
    delete next.exclusions;
  }

  writeConfig(next);
}

function setConfigField(name: string, value: unknown) {
  writeConfig({ ...configDraft.value, [name]: value });
}

function toNodeField(field: UiField): NodeFieldMetadata {
  return { ...field, location: { base: "parameters", path: [] } };
}

watch(
  () => workflows.triggerEditorOpen,
  async (open) => {
    if (!open) {
      return;
    }

    triggerPickerOpen.value = false;
    await nextTick();
    editor.value?.focus();
  },
);
</script>

<style scoped>
.trigger-section {
  display: grid;
  gap: 14px;
}
.trigger-section-heading {
  align-items: center;
}
.trigger-heading-line {
  display: flex;
  align-items: center;
  gap: 8px;
}
.trigger-heading-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.trigger-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 22px;
  padding: 0 7px;
  border-radius: 999px;
  background: var(--surface-subtle);
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
}
.trigger-picker {
  display: grid;
  gap: 14px;
  padding: 16px;
  border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border-subtle));
  border-radius: calc(var(--radius) + 2px);
  background: color-mix(in srgb, var(--accent-soft) 18%, var(--surface));
  box-shadow: 0 12px 32px color-mix(in srgb, var(--text) 8%, transparent);
}
.trigger-picker-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.trigger-picker-heading h4,
.trigger-picker-heading p {
  margin: 0;
}
.trigger-picker-heading h4 {
  font-size: 13px;
  color: var(--text);
}
.trigger-picker-heading p {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 11px;
}
.trigger-kind-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}
.trigger-kind-option {
  display: grid;
  grid-template-columns: 36px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  min-height: 72px;
  padding: 11px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text-muted);
  text-align: left;
  cursor: pointer;
  transition:
    border-color 120ms ease,
    background 120ms ease,
    transform 120ms ease;
}
.trigger-kind-option:hover {
  border-color: var(--accent);
  background: var(--surface-hover);
  transform: translateY(-1px);
}
.trigger-kind-icon,
.trigger-card-icon,
.trigger-editor-icon,
.trigger-empty-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  background: var(--accent-soft);
  color: var(--accent);
}
.trigger-kind-icon {
  width: 36px;
  height: 36px;
}
.trigger-kind-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}
.trigger-kind-copy strong {
  color: var(--text);
  font-size: 12px;
}
.trigger-kind-copy small {
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.35;
}
.trigger-callout {
  padding: 12px 14px;
  border-radius: var(--radius);
  background: var(--surface-subtle);
}
.trigger-empty-state {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  padding: 18px;
  border: 1px dashed var(--border-strong);
  border-radius: calc(var(--radius) + 2px);
  background: var(--surface-subtle);
}
.trigger-empty-icon {
  width: 42px;
  height: 42px;
}
.trigger-empty-state h4,
.trigger-empty-state p {
  margin: 0;
}
.trigger-empty-state h4 {
  color: var(--text);
  font-size: 13px;
}
.trigger-empty-state p {
  max-width: 560px;
  margin-top: 4px;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.45;
}
.trigger-card-list {
  display: grid;
  gap: 8px;
}
.trigger-card {
  display: grid;
  grid-template-columns: 38px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  padding: 12px 13px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  transition:
    border-color 120ms ease,
    box-shadow 120ms ease;
}
.trigger-card:hover {
  border-color: var(--border-strong);
  box-shadow: 0 4px 18px color-mix(in srgb, var(--text) 5%, transparent);
}
.trigger-card.is-disabled {
  background: var(--surface-subtle);
}
.trigger-card-icon {
  width: 38px;
  height: 38px;
}
.trigger-card.is-disabled .trigger-card-icon {
  background: var(--surface);
  color: var(--text-faint);
}
.trigger-card-content {
  display: grid;
  gap: 4px;
  min-width: 0;
}
.trigger-card-title {
  display: flex;
  align-items: center;
  gap: 8px;
}
.trigger-card-title strong {
  color: var(--text);
  font-size: 12px;
}
.trigger-card-summary {
  margin: 0;
  overflow: hidden;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.trigger-card-meta {
  color: var(--text-muted);
  font-size: 10.5px;
}
.trigger-card-meta span {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.trigger-card-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.trigger-editor {
  display: grid;
  gap: 20px;
  margin-top: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
}
.trigger-editor-header {
  display: grid;
  justify-items: start;
  gap: 14px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--border-subtle);
}
.trigger-back-button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 650;
  cursor: pointer;
}
.trigger-back-button:hover {
  color: var(--accent);
}
.trigger-editor-heading {
  display: flex;
  align-items: center;
  gap: 12px;
}
.trigger-editor-icon {
  width: 42px;
  height: 42px;
}
.trigger-editor-heading h3,
.trigger-editor-heading p {
  margin: 0;
}
.trigger-editor-heading h3 {
  font-size: 16px;
}
.trigger-editor-heading p:last-child {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 11px;
}
.trigger-editor-topline {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(280px, 0.9fr);
  gap: 12px;
}
.trigger-type-field {
  display: grid;
  gap: 6px;
}
.trigger-type-field > span {
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
}
.trigger-type-field small {
  color: var(--text-muted);
  font-size: 10.5px;
}
.trigger-status-control {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 11px 13px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  cursor: pointer;
}
.trigger-status-copy {
  display: grid;
  gap: 3px;
}
.trigger-status-copy strong {
  color: var(--text);
  font-size: 12px;
}
.trigger-status-copy small {
  color: var(--text-muted);
  font-size: 10.5px;
  line-height: 1.35;
}
.switch {
  position: relative;
  display: inline-flex;
  flex: 0 0 auto;
  width: 34px;
  height: 20px;
}
.switch input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
}
.switch > span {
  width: 100%;
  border-radius: 999px;
  background: var(--border-strong);
  transition: background 120ms ease;
}
.switch > span::after {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: white;
  box-shadow: 0 1px 3px rgb(0 0 0 / 25%);
  content: "";
  transition: transform 120ms ease;
}
.switch.is-on > span {
  background: var(--success-fg);
}
.switch.is-on > span::after {
  transform: translateX(14px);
}
.switch input:focus-visible + span {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.trigger-editor-section {
  display: grid;
  gap: 12px;
}
.trigger-step-heading {
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr);
  align-items: start;
  gap: 9px;
}
.trigger-step-heading > span {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 11px;
  font-weight: 800;
}
.trigger-step-heading h4,
.trigger-step-heading p {
  margin: 0;
}
.trigger-step-heading h4 {
  color: var(--text);
  font-size: 13px;
}
.trigger-step-heading h4 em {
  margin-left: 5px;
  color: var(--text-faint);
  font-size: 10px;
  font-style: normal;
  font-weight: 600;
}
.trigger-step-heading p {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 11px;
}
.trigger-fields-card {
  padding: 14px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
}
.trigger-blackout-section {
  padding: 14px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}
.trigger-blackout-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  width: fit-content;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
}
.trigger-ready-state {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  padding: 14px;
  border: 1px solid color-mix(in srgb, var(--success-fg) 30%, var(--border-subtle));
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--success-bg) 50%, var(--surface));
}
.trigger-ready-state > span {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: var(--success-bg);
  color: var(--success-fg);
}
.trigger-ready-state strong {
  color: var(--text);
  font-size: 12px;
}
.trigger-ready-state p {
  margin: 2px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}
.trigger-advanced {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}
.trigger-advanced > summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  cursor: pointer;
  list-style: none;
}
.trigger-advanced > summary::-webkit-details-marker {
  display: none;
}
.trigger-advanced > summary > span {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
}
.trigger-advanced > summary small {
  color: var(--text-faint);
  font-size: 10.5px;
}
.trigger-advanced[open] > summary {
  border-bottom: 1px solid var(--border-subtle);
}
.trigger-advanced-content {
  display: grid;
  gap: 14px;
  padding: 14px;
}
.trigger-window {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(220px, 0.65fr);
  align-items: end;
  gap: 14px;
  padding: 0;
  border: 0;
  background: transparent;
}
.trigger-window-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.trigger-window-heading h4,
.trigger-window-heading p {
  margin: 0;
}
.trigger-window-heading h4 {
  font-size: 12px;
}
.trigger-window-heading p,
.trigger-window-heading > span {
  color: var(--text-muted);
  font-size: 10.5px;
}
.trigger-window-heading > span {
  display: none;
}
.trigger-window > label {
  display: grid;
  gap: 5px;
}
.trigger-window > label > span {
  color: var(--text-subtle);
  font-size: 11px;
  font-weight: 650;
}
.trigger-json-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.trigger-json-grid .form-field {
  display: grid;
  gap: 6px;
  min-width: 0;
}
.trigger-json-grid .form-field > small {
  color: var(--text-muted);
  font-size: 10.5px;
}
.trigger-editor-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 14px;
  border-top: 1px solid var(--border-subtle);
}
@media (max-width: 760px) {
  .trigger-section-heading,
  .trigger-picker-heading {
    align-items: stretch;
    flex-direction: column;
  }
  .trigger-heading-actions {
    justify-content: flex-start;
  }
  .trigger-kind-grid,
  .trigger-editor-topline,
  .trigger-json-grid,
  .trigger-window {
    grid-template-columns: minmax(0, 1fr);
  }
  .trigger-empty-state {
    grid-template-columns: 42px minmax(0, 1fr);
  }
  .trigger-empty-state .btn {
    grid-column: 1 / -1;
  }
  .trigger-card {
    grid-template-columns: 38px minmax(0, 1fr);
  }
  .trigger-card-actions {
    grid-column: 2;
    justify-content: flex-start;
  }
  .trigger-advanced > summary {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
