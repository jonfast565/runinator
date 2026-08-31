<template>
  <div class="modal-backdrop">
    <form class="modal workflow-settings-modal" @submit.prevent="saveAndClose">
      <header class="modal-header">
        <div class="workflow-settings-title">
          <div class="flex items-center gap-1">
            <h2>Workflow Settings</h2>
            <HelpBubble
              text="Manage workflow identity, ownership, and triggers. Changes are saved with the workflow."
              label="About workflow settings"
            />
          </div>
          <p>{{ workflowPath(workflows.workflowDraft) }}</p>
        </div>
        <button
          type="button"
          class="btn-close"
          aria-label="Close workflow settings"
          @click="workflows.closeWorkflowSettings"
        >
          <Icon name="close" :size="16" />
        </button>
      </header>

      <div class="workflow-settings-body">
        <section class="form-section workflow-settings-section workflow-settings-identity">
          <div class="workflow-settings-section-heading">
            <div>
              <p class="workflow-settings-eyebrow">Definition</p>
              <h3>Identity &amp; release</h3>
              <p class="hint">These values identify this workflow in packs, links, and releases.</p>
            </div>
            <span
              class="workflow-state-pill"
              :class="workflows.workflowDraft.enabled ? 'is-enabled' : 'is-disabled'"
            >
              {{ workflows.workflowDraft.enabled ? "Enabled" : "Disabled" }}
            </span>
          </div>
          <div class="workflow-settings-grid">
            <label
              class="workflow-settings-name"
              :class="{ 'has-error': workflowSettingsValidation.name }"
            >
              <span>Name</span>
              <input
                v-model="workflows.workflowDraft.name"
                type="text"
                required
                maxlength="256"
                autocomplete="off"
                :aria-invalid="Boolean(workflowSettingsValidation.name)"
                aria-describedby="workflow-name-help workflow-name-error"
                @input="workflows.markWorkflowDirty"
              />
              <small id="workflow-name-help"
                >A clear display name for people reading the workflow.</small
              >
              <small
                v-if="workflowSettingsValidation.name"
                id="workflow-name-error"
                class="field-error"
                role="alert"
              >
                {{ workflowSettingsValidation.name }}
              </small>
            </label>
            <label :class="{ 'has-error': workflowSettingsValidation.namespace }">
              <span>Namespace</span>
              <input
                v-model="workflows.workflowDraft.namespace"
                type="text"
                required
                maxlength="256"
                :pattern="namespacePattern"
                title="Use dot-separated identifiers, for example acme.delivery."
                placeholder="acme.delivery"
                autocomplete="off"
                :aria-invalid="Boolean(workflowSettingsValidation.namespace)"
                aria-describedby="workflow-namespace-help workflow-namespace-error"
                @input="workflows.markWorkflowDirty"
              />
              <small id="workflow-namespace-help"
                >Dot-separated identifiers, such as <code>acme.delivery</code>.</small
              >
              <small
                v-if="workflowSettingsValidation.namespace"
                id="workflow-namespace-error"
                class="field-error"
                role="alert"
              >
                {{ workflowSettingsValidation.namespace }}
              </small>
            </label>
            <label :class="{ 'has-error': workflowSettingsValidation.key }">
              <span>Stable key</span>
              <input
                v-model="workflows.workflowDraft.key"
                type="text"
                required
                maxlength="256"
                :pattern="REXRAP_IDENTIFIER_PATTERN"
                title="Start with a letter or underscore and use only letters, numbers, and underscores."
                placeholder="release_train"
                autocomplete="off"
                :aria-invalid="Boolean(workflowSettingsValidation.key)"
                aria-describedby="workflow-key-help workflow-key-error"
                @input="workflows.markWorkflowDirty"
              />
              <small id="workflow-key-help"
                >Used in the durable workflow path; changing it can break links.</small
              >
              <small
                v-if="workflowSettingsValidation.key"
                id="workflow-key-error"
                class="field-error"
                role="alert"
              >
                {{ workflowSettingsValidation.key }}
              </small>
            </label>
            <label :class="{ 'has-error': workflowSettingsValidation.version }">
              <span>Version</span>
              <input
                v-model="workflows.workflowDraft.version"
                type="text"
                required
                :pattern="WORKFLOW_VERSION_PATTERN"
                title="Use semantic versioning, for example 1.0.0."
                placeholder="1.0.0"
                inputmode="numeric"
                :aria-invalid="Boolean(workflowSettingsValidation.version)"
                aria-describedby="workflow-version-help workflow-version-error"
                @input="workflows.markWorkflowDirty"
              />
              <small id="workflow-version-help"
                >Use major.minor.patch, for example <code>1.0.0</code>.</small
              >
              <small
                v-if="workflowSettingsValidation.version"
                id="workflow-version-error"
                class="field-error"
                role="alert"
              >
                {{ workflowSettingsValidation.version }}
              </small>
            </label>
            <div class="workflow-enabled-control">
              <label class="checkbox">
                <input
                  v-model="workflows.workflowDraft.enabled"
                  type="checkbox"
                  @change="workflows.markWorkflowDirty"
                />
                <span>Enable workflow</span>
              </label>
              <p>Disabled workflows stay available for editing but cannot start new runs.</p>
            </div>
          </div>
        </section>

        <section class="form-section workflow-settings-section ownership-section">
          <div class="workflow-settings-section-heading">
            <div>
              <p class="workflow-settings-eyebrow">Access</p>
              <h3>Ownership</h3>
            </div>
            <HelpBubble
              text="Scoping a workflow to an organization limits its runs and visibility to that organization's members. Share with individuals or teams from the Share dialog. Only organization admins can change ownership."
              label="About workflow ownership"
            />
          </div>
          <p v-if="!workflows.workflowDraft.id" class="hint">
            Save the workflow before assigning an owner.
          </p>
          <template v-else>
            <div class="workflow-settings-card">
              <label>
                <span>Owning organization</span>
                <select v-model="ownerOrgId" :disabled="ownerSaving" @change="saveOwner">
                  <option value="">Platform-global (none)</option>
                  <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
                    {{ m.org.name }}
                  </option>
                </select>
              </label>
              <p class="hint">
                Organization members can discover and run this workflow according to their
                permissions.
              </p>
            </div>
          </template>
        </section>

        <section class="form-section workflow-settings-section trigger-section">
          <div class="workflow-settings-section-heading">
            <div>
              <p class="workflow-settings-eyebrow">Automation</p>
              <h3>Triggers</h3>
              <p class="hint">Choose when this workflow should start automatically.</p>
            </div>
            <div class="trigger-controls">
              <button
                type="button"
                class="btn"
                :disabled="!workflows.canManageWorkflowTriggers"
                @click="workflows.refreshWorkflowTriggers"
              >
                Refresh
              </button>
              <select
                v-model="newTriggerKind"
                aria-label="Trigger type to add"
                :disabled="!workflows.canManageWorkflowTriggers || !catalogMetadata.loaded"
              >
                <option
                  v-for="kind in catalogMetadata.triggerKinds"
                  :key="kind.kind"
                  :value="kind.kind"
                >
                  {{ kind.label }} trigger
                </option>
              </select>
              <button
                type="button"
                class="btn btn-primary"
                :disabled="!workflows.canManageWorkflowTriggers || !catalogMetadata.loaded"
                @click="addWorkflowTrigger"
              >
                Add trigger
              </button>
            </div>
          </div>

          <p v-if="!workflows.canManageWorkflowTriggers" class="hint">
            Save the workflow before adding triggers.
          </p>
          <p v-else-if="!catalogMetadata.loaded" class="hint catalog-loading-hint">
            <LoadingSpinner size="sm" label="Loading trigger types" />
            Loading trigger types…
          </p>
          <p
            v-else-if="workflows.workflowTriggers.length === 0"
            class="hint workflow-settings-empty-state"
          >
            No automatic triggers yet. Add one to schedule this workflow or start it after another
            workflow finishes.
          </p>

          <div v-else class="trigger-table-wrap">
            <DataTable bare compact>
              <thead>
                <tr>
                  <th>Kind</th>
                  <th>State</th>
                  <th>Schedule</th>
                  <th>Next run</th>
                  <th><span class="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="trigger in workflows.workflowTriggers"
                  :key="trigger.id ?? `${trigger.kind}-${trigger.workflow_id}`"
                  :class="{ muted: !trigger.enabled }"
                >
                  <td>
                    <span class="trigger-kind-pill">{{ triggerKindLabel(trigger.kind) }}</span>
                  </td>
                  <td>
                    <span
                      class="workflow-state-pill"
                      :class="trigger.enabled ? 'is-enabled' : 'is-disabled'"
                    >
                      {{ trigger.enabled ? "Enabled" : "Disabled" }}
                    </span>
                  </td>
                  <td>{{ workflows.triggerCronSummary(trigger) || "Runs on demand" }}</td>
                  <td>{{ trigger.next_execution ?? "—" }}</td>
                  <td class="row-actions">
                    <button
                      type="button"
                      class="btn"
                      @click="workflows.editWorkflowTrigger(trigger)"
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      class="btn btn-danger"
                      @click="workflows.deleteSelectedWorkflowTrigger(trigger)"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              </tbody>
            </DataTable>
          </div>

          <section
            v-if="workflows.triggerEditorOpen"
            class="trigger-editor"
            aria-labelledby="trigger-editor-title"
          >
            <header class="trigger-editor-header">
              <div>
                <p class="workflow-settings-eyebrow">
                  {{ workflows.triggerEditorCreating ? "New trigger" : "Edit trigger" }}
                </p>
                <h3 id="trigger-editor-title">
                  {{ triggerKindLabel(workflows.triggerDraft.kind) }} trigger
                </h3>
              </div>
              <button type="button" class="btn" @click="workflows.closeTriggerEditor">
                Cancel
              </button>
            </header>
            <div class="trigger-editor-grid">
              <label>
                <span>Trigger type</span>
                <select
                  v-model="workflows.triggerDraft.kind"
                  :disabled="!catalogMetadata.loaded"
                  @change="workflows.setTriggerKind(workflows.triggerDraft.kind)"
                >
                  <option v-if="!catalogMetadata.loaded" value="" disabled>
                    Loading trigger types…
                  </option>
                  <option
                    v-for="kind in catalogMetadata.triggerKinds"
                    :key="kind.kind"
                    :value="kind.kind"
                  >
                    {{ kind.label }}
                  </option>
                </select>
              </label>
              <div class="workflow-enabled-control trigger-enabled-control">
                <label class="checkbox"
                  ><input v-model="workflows.triggerDraft.enabled" type="checkbox" />
                  <span>Enable trigger</span></label
                >
                <p>Keep it disabled while you finish configuring it.</p>
              </div>
            </div>
            <div class="trigger-window">
              <div class="trigger-window-heading">
                <div>
                  <h4>Timing window</h4>
                  <p>Optionally set the next run or pause runs during a blackout period.</p>
                </div>
                <span>Local time</span>
              </div>
              <div class="trigger-datetime-grid">
                <label :class="{ 'has-error': triggerValidation.errors.nextExecution }">
                  <span>Next execution</span>
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
                <label :class="{ 'has-error': triggerValidation.errors.blackoutStart }">
                  <span>Blackout starts</span>
                  <input
                    v-model="workflows.triggerDraft.blackout_start"
                    type="datetime-local"
                    :aria-invalid="Boolean(triggerValidation.errors.blackoutStart)"
                    aria-describedby="trigger-blackout-start-error"
                  />
                  <small
                    v-if="triggerValidation.errors.blackoutStart"
                    id="trigger-blackout-start-error"
                    class="field-error"
                  >
                    {{ triggerValidation.errors.blackoutStart }}
                  </small>
                </label>
                <label :class="{ 'has-error': triggerValidation.errors.blackoutEnd }">
                  <span>Blackout ends</span>
                  <input
                    v-model="workflows.triggerDraft.blackout_end"
                    type="datetime-local"
                    :aria-invalid="Boolean(triggerValidation.errors.blackoutEnd)"
                    aria-describedby="trigger-blackout-end-error"
                  />
                  <small
                    v-if="triggerValidation.errors.blackoutEnd"
                    id="trigger-blackout-end-error"
                    class="field-error"
                  >
                    {{ triggerValidation.errors.blackoutEnd }}
                  </small>
                </label>
              </div>
            </div>
            <div class="trigger-json-grid">
              <div
                class="form-field"
                :class="{ 'has-error': triggerValidation.errors.configuration }"
              >
                <span class="form-field-label">Configuration</span>
                <template v-if="triggerKindMeta && triggerKindMeta.fields.length">
                  <div class="trigger-field-list">
                    <div
                      v-for="field in triggerKindMeta.fields"
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
                </template>
                <p
                  v-else-if="catalogMetadata.loading || !catalogMetadata.loaded"
                  class="hint catalog-loading-hint"
                >
                  <LoadingSpinner size="sm" label="Loading trigger metadata" />
                  Loading trigger metadata…
                </p>
                <JsonEditor v-else v-model="workflows.triggerJson.configuration" />
                <small v-if="triggerValidation.errors.configuration" class="field-error">
                  {{ triggerValidation.errors.configuration }}
                </small>
              </div>
              <div class="form-field" :class="{ 'has-error': triggerValidation.errors.metadata }">
                <span class="form-field-label">Metadata</span>
                <JsonEditor v-model="workflows.triggerJson.metadata" />
                <small v-if="triggerValidation.errors.metadata" class="field-error">
                  {{ triggerValidation.errors.metadata }}
                </small>
              </div>
            </div>
            <p v-if="workflows.triggerEditorError" class="error m-0 text-xs" role="alert">
              {{ workflows.triggerEditorError }}
            </p>
            <div class="modal-actions trigger-editor-actions">
              <button type="button" class="btn" @click="workflows.closeTriggerEditor">
                Cancel
              </button>
              <button
                type="button"
                class="btn btn-primary"
                :disabled="Boolean(triggerValidation.error)"
                @click="workflows.submitWorkflowTrigger"
              >
                Save trigger
              </button>
            </div>
          </section>
        </section>

        <WorkflowRevisionsPanel
          :workflow-id="workflows.workflowDraft.id ?? null"
          @restored="onRevisionRestored"
        />
      </div>

      <footer class="modal-actions workflow-settings-actions">
        <p v-if="workflowSettingsError" class="workflow-settings-validation-summary" role="alert">
          Review the highlighted workflow fields before saving.
        </p>
        <div class="workflow-settings-actions-buttons">
          <button
            type="button"
            class="btn btn-danger"
            :disabled="!workflows.workflowDraft.id"
            @click="workflows.deleteSelectedWorkflow"
          >
            Delete workflow
          </button>
          <button
            type="button"
            class="btn"
            :disabled="!workflows.workflowDraft.id || workflows.isDirty"
            @click="workflows.duplicateSelectedWorkflow('minor')"
          >
            Duplicate version
          </button>
          <button
            type="submit"
            class="btn btn-primary"
            :disabled="saving || Boolean(workflowSettingsError)"
          >
            <LoadingSpinner v-if="saving" size="sm" label="Saving workflow" />
            {{ saving ? "Saving…" : workflows.isDirty ? "Save & Close" : "Done" }}
          </button>
        </div>
      </footer>
    </form>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useOrgsStore } from "../../../ui/adapters/pinia/orgs";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useCatalogMetadataStore } from "../../../ui/adapters/pinia/catalogMetadata";
import { workflowSharingService } from "../../../core/services";
import type {
  NodeFieldMetadata,
  UiField,
  WorkflowDefinition,
  WorkflowTriggerKind,
} from "../../../core/domain/models";
import {
  artifactIdentityPath,
  REXRAP_IDENTIFIER_PATTERN,
  workflowPath,
  workflowSettingsErrors,
  WORKFLOW_VERSION_PATTERN,
} from "../../../core/domain/models";
import { validateTriggerEditor } from "../../../core/workflow/trigger-validation";
import JsonEditor from "../shared/JsonEditor.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";
import WorkflowRevisionsPanel from "./WorkflowRevisionsPanel.vue";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const catalogMetadata = useCatalogMetadataStore();
const namespacePattern = `${REXRAP_IDENTIFIER_PATTERN}(\\.${REXRAP_IDENTIFIER_PATTERN})*`;
const workflowSettingsValidation = computed(() => {
  const errors = workflowSettingsErrors(workflows.workflowDraft);

  if (errors.name || errors.namespace || errors.key || errors.version) {
    return errors;
  }

  const path = artifactIdentityPath(workflows.workflowDraft);
  const targetOrgId = workflows.workflowDraft.id
    ? (workflows.workflowDraft.org_id ?? null)
    : (orgs.activeOrgId ?? null);
  return workflows.workflows.some(
    (workflow) =>
      workflow.id != null &&
      workflow.id !== workflows.workflowDraft.id &&
      (workflow.org_id ?? null) === targetOrgId &&
      (artifactIdentityPath(workflow) === path ||
        workflow.key === workflows.workflowDraft.key?.trim()),
  )
    ? {
        ...errors,
        key: `The stable key ${workflows.workflowDraft.key?.trim() ?? path} is already used in this scope.`,
      }
    : errors;
});
const workflowSettingsError = computed(() => {
  const errors = workflowSettingsValidation.value;
  return errors.name || errors.namespace || errors.key || errors.version;
});

const ownerOrgId = ref<string>(workflows.workflowDraft.org_id ?? "");
const ownerSaving = ref(false);
const newTriggerKind = ref<WorkflowTriggerKind>("cron");

const triggerKindMeta = computed(() => catalogMetadata.triggerKind(workflows.triggerDraft.kind));
const triggerValidation = computed(() =>
  validateTriggerEditor(
    workflows.triggerDraft,
    workflows.triggerJson.configuration,
    workflows.triggerJson.metadata,
    triggerKindMeta.value,
  ),
);

// the trigger json is the single copy of the configuration; the per-field editors read it back out
// on every render rather than keeping a snapshot, so opening a second trigger cannot show — or save
// — the first one's values.
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

function setConfigField(name: string, value: unknown) {
  workflows.triggerJson.configuration = JSON.stringify(
    { ...configDraft.value, [name]: value },
    null,
    2,
  );
}

// adapts a UiField to NodeFieldMetadata for CatalogFieldEditor (location is unused by the editor).
function toNodeField(f: UiField): NodeFieldMetadata {
  return { ...f, location: { base: "parameters", path: [] } };
}

function triggerKindLabel(kind: WorkflowTriggerKind): string {
  return catalogMetadata.triggerKind(kind)?.label ?? kind;
}

function addWorkflowTrigger() {
  workflows.addWorkflowTrigger(newTriggerKind.value);
}

// keep the owner select in sync when the edited workflow changes.
watch(
  () => workflows.workflowDraft.id,
  () => {
    ownerOrgId.value = workflows.workflowDraft.org_id ?? "";
  },
);

watch(
  () => catalogMetadata.triggerKinds,
  (kinds) => {
    if (kinds.length && !kinds.some((kind) => kind.kind === newTriggerKind.value)) {
      newTriggerKind.value = kinds[0].kind;
    }
  },
  { immediate: true },
);

// name, version, and enabled only mark the draft dirty; the triggers beside them save through their
// own endpoint the moment they are submitted. leaving the two halves of one dialog on different
// rules is what made an unchecked "Enabled" look applied while the workflow kept running on its
// schedule, so closing commits the draft too.
const saving = ref(false);

function draftNeedsSave(): boolean {
  return workflows.isDirty;
}

async function saveAndClose() {
  if (workflowSettingsError.value) {
    return;
  }

  if (draftNeedsSave()) {
    saving.value = true;

    try {
      await workflows.saveSelectedWorkflow();
    } catch {
      // the operation runner already raised the error toast. keep the dialog open so the edit is
      // still there to retry rather than closing over a save that did not happen.
      return;
    } finally {
      saving.value = false;
    }

    // a save can also decline without throwing; the draft staying dirty is how that reads.
    if (draftNeedsSave()) {
      return;
    }
  }

  workflows.closeWorkflowSettings();
}

async function saveOwner() {
  const id = workflows.workflowDraft.id;

  if (!id) {
    return;
  }

  ownerSaving.value = true;

  try {
    const updated = await workflowSharingService.setOwner(id, ownerOrgId.value || null);
    workflows.workflowDraft.org_id = updated.org_id ?? null;
    app.setStatus("Workflow ownership updated");
  } catch (error) {
    app.setError(String(error));
    // revert the select to the stored value on failure.
    ownerOrgId.value = workflows.workflowDraft.org_id ?? "";
  } finally {
    ownerSaving.value = false;
  }
}

// a rollback changed the stored definition under the open editor, so reset the draft to it rather
// than leaving the now-stale one in place to be saved back over the restore.
async function onRevisionRestored(restored: WorkflowDefinition) {
  await workflows.selectWorkflow(restored);
  await workflows.refreshWorkflows();
}

if (!orgs.memberships.length) {
  void orgs.refresh();
}
</script>
