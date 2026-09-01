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

        <WorkflowTriggersPanel />

        <WorkflowRevisionsPanel
          :workflow-id="workflows.workflowDraft.id ?? null"
          @restored="onRevisionRestored"
        />
      </div>

      <footer v-if="!workflows.triggerEditorOpen" class="modal-actions workflow-settings-actions">
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
import { workflowSharingService } from "../../../core/services";
import type { WorkflowDefinition } from "../../../core/domain/models";
import {
  artifactIdentityPath,
  REXRAP_IDENTIFIER_PATTERN,
  workflowPath,
  workflowSettingsErrors,
  WORKFLOW_VERSION_PATTERN,
} from "../../../core/domain/models";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import WorkflowRevisionsPanel from "./WorkflowRevisionsPanel.vue";
import WorkflowTriggersPanel from "./WorkflowTriggersPanel.vue";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
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

// keep the owner select in sync when the edited workflow changes.
watch(
  () => workflows.workflowDraft.id,
  () => {
    ownerOrgId.value = workflows.workflowDraft.org_id ?? "";
  },
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
