<template>
  <div class="modal-backdrop">
    <form class="modal w-[min(1040px,100%)]" @submit.prevent="saveAndClose">
      <header class="modal-header">
        <div class="flex items-center gap-1">
          <h2>Workflow Settings</h2>
          <HelpBubble
            text="Manage workflow identity, ownership, and triggers. Changes are saved with the workflow."
            label="About workflow settings"
          />
        </div>
        <button type="button" @click="workflows.closeWorkflowSettings">Close</button>
      </header>

      <section class="form-section">
        <div class="form-grid">
          <label
            >Name
            <input
              v-model="workflows.workflowDraft.name"
              type="text"
              required
              maxlength="256"
              @input="workflows.markWorkflowDirty"
          /></label>
          <label
            >Namespace
            <input
              v-model="workflows.workflowDraft.namespace"
              type="text"
              required
              maxlength="256"
              :pattern="namespacePattern"
              title="Use dot-separated identifiers, for example acme.delivery."
              placeholder="acme.delivery"
              @input="workflows.markWorkflowDirty"
          /></label>
          <label
            >Stable key
            <input
              v-model="workflows.workflowDraft.key"
              type="text"
              required
              maxlength="256"
              :pattern="REXRAP_IDENTIFIER_PATTERN"
              title="Start with a letter or underscore and use only letters, numbers, and underscores."
              placeholder="release_train"
              @input="workflows.markWorkflowDirty"
          /></label>
          <label
            >Version
            <input
              v-model="workflows.workflowDraft.version"
              type="text"
              placeholder="1.0.0"
              pattern="\d+\.\d+\.\d+"
              @input="workflows.markWorkflowDirty"
          /></label>
          <label class="checkbox"
            ><input
              v-model="workflows.workflowDraft.enabled"
              type="checkbox"
              @change="workflows.markWorkflowDirty"
            />
            Enabled</label
          >
        </div>
        <p v-if="workflowIdentityError" class="error mb-0 mt-2 text-xs" role="alert">
          {{ workflowIdentityError }}
        </p>
      </section>

      <section class="form-section ownership-section">
        <div class="section-toolbar">
          <div class="flex items-center gap-1">
            <h3>Ownership</h3>
            <HelpBubble
              text="Scoping a workflow to an organization limits its runs and visibility to that organization's members. Share with individuals or teams from the Share dialog. Only organization admins can change ownership."
              label="About workflow ownership"
            />
          </div>
        </div>
        <p v-if="!workflows.workflowDraft.id" class="hint">
          Save the workflow before assigning an owner.
        </p>
        <template v-else>
          <div class="form-grid">
            <label>
              Owning organization
              <select v-model="ownerOrgId" :disabled="ownerSaving" @change="saveOwner">
                <option value="">Platform-global (none)</option>
                <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
                  {{ m.org.name }}
                </option>
              </select>
            </label>
          </div>
        </template>
      </section>

      <section class="form-section trigger-section">
        <div class="section-toolbar">
          <h3>Triggers</h3>
          <div class="section-actions">
            <button
              type="button"
              :disabled="!workflows.canManageWorkflowTriggers"
              @click="workflows.refreshWorkflowTriggers"
            >
              Refresh
            </button>
            <template v-if="catalogMetadata.loaded">
              <button
                v-for="kind in catalogMetadata.triggerKinds"
                :key="kind.kind"
                type="button"
                :disabled="!workflows.canManageWorkflowTriggers"
                @click="workflows.addWorkflowTrigger(kind.kind)"
              >
                New {{ kind.label }}
              </button>
            </template>
            <p v-else class="hint catalog-loading-hint">
              <LoadingSpinner size="sm" label="Loading trigger types" />
              Loading trigger types…
            </p>
          </div>
        </div>

        <p v-if="!workflows.canManageWorkflowTriggers" class="hint">
          Save the workflow before adding triggers.
        </p>
        <p v-else-if="workflows.workflowTriggers.length === 0" class="hint">
          No triggers configured.
        </p>

        <div v-else class="trigger-table-wrap">
          <DataTable bare compact>
            <thead>
              <tr>
                <th>Kind</th>
                <th>State</th>
                <th>Schedule</th>
                <th>Next</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="trigger in workflows.workflowTriggers"
                :key="trigger.id ?? `${trigger.kind}-${trigger.workflow_id}`"
                :class="{ muted: !trigger.enabled }"
              >
                <td>{{ trigger.kind }}</td>
                <td>{{ trigger.enabled ? "enabled" : "disabled" }}</td>
                <td>{{ workflows.triggerCronSummary(trigger) || "-" }}</td>
                <td>{{ trigger.next_execution ?? "-" }}</td>
                <td class="row-actions">
                  <button type="button" @click="workflows.editWorkflowTrigger(trigger)">
                    Edit
                  </button>
                  <button type="button" @click="workflows.deleteSelectedWorkflowTrigger(trigger)">
                    Delete
                  </button>
                </td>
              </tr>
            </tbody>
          </DataTable>
        </div>

        <div v-if="workflows.triggerEditorOpen" class="trigger-editor">
          <div class="section-toolbar">
            <h3>{{ workflows.triggerEditorCreating ? "New Trigger" : "Edit Trigger" }}</h3>
            <button type="button" @click="workflows.closeTriggerEditor">Cancel</button>
          </div>
          <div class="form-grid">
            <label>
              Kind
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
            <label class="checkbox"
              ><input v-model="workflows.triggerDraft.enabled" type="checkbox" /> Enabled</label
            >
            <label
              >Next Execution
              <input v-model="workflows.triggerDraft.next_execution" type="datetime-local"
            /></label>
            <label
              >Blackout Start
              <input v-model="workflows.triggerDraft.blackout_start" type="datetime-local"
            /></label>
            <label
              >Blackout End
              <input v-model="workflows.triggerDraft.blackout_end" type="datetime-local"
            /></label>
          </div>
          <div class="trigger-json-grid">
            <div class="form-field">
              <span class="form-field-label">Configuration</span>
              <!-- when the catalog provides fields for this trigger kind, render per-field editors. -->
              <template v-if="triggerKindMeta && triggerKindMeta.fields.length">
                <div class="trigger-field-list">
                  <CatalogFieldEditor
                    v-for="field in triggerKindMeta.fields"
                    :key="field.name"
                    :field="toNodeField(field)"
                    :model-value="configDraft[field.name]"
                    :workflows="workflows.workflows"
                    @update:model-value="setConfigField(field.name, $event)"
                  />
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
            </div>
            <div class="form-field">
              <span class="form-field-label">Metadata</span>
              <JsonEditor v-model="workflows.triggerJson.metadata" />
            </div>
          </div>
          <p v-if="workflows.triggerEditorError" class="error m-0 text-xs">
            {{ workflows.triggerEditorError }}
          </p>
          <div class="modal-actions">
            <button type="button" @click="workflows.closeTriggerEditor">Cancel</button>
            <button type="button" @click="workflows.submitWorkflowTrigger">Save Trigger</button>
          </div>
        </div>
      </section>

      <WorkflowRevisionsPanel
        :workflow-id="workflows.workflowDraft.id ?? null"
        @restored="onRevisionRestored"
      />

      <div class="modal-actions">
        <button
          type="button"
          class="btn btn-danger"
          :disabled="!workflows.workflowDraft.id"
          @click="workflows.deleteSelectedWorkflow"
        >
          Delete Workflow
        </button>
        <button
          type="button"
          :disabled="!workflows.workflowDraft.id || workflows.isDirty"
          @click="workflows.duplicateSelectedWorkflow('minor')"
        >
          Duplicate (bump version)
        </button>
        <button type="submit" :disabled="saving || Boolean(workflowIdentityError)">
          <LoadingSpinner v-if="saving" size="sm" label="Saving workflow" />
          {{ saving ? "Saving…" : workflows.isDirty ? "Save & Close" : "Done" }}
        </button>
      </div>
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
import type { NodeFieldMetadata, UiField } from "../../../core/domain/models";
import type { WorkflowDefinition } from "../../../core/domain/models";
import {
  artifactIdentityError,
  artifactIdentityPath,
  REXRAP_IDENTIFIER_PATTERN,
} from "../../../core/domain/models";
import JsonEditor from "../shared/JsonEditor.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";
import WorkflowRevisionsPanel from "./WorkflowRevisionsPanel.vue";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const catalogMetadata = useCatalogMetadataStore();
const namespacePattern = `${REXRAP_IDENTIFIER_PATTERN}(\\.${REXRAP_IDENTIFIER_PATTERN})*`;
const workflowIdentityError = computed(() => {
  const invalid = artifactIdentityError(workflows.workflowDraft);

  if (invalid) {
    return invalid;
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
    ? `The stable key ${workflows.workflowDraft.key?.trim() ?? path} is already used in this scope.`
    : "";
});

const ownerOrgId = ref<string>(workflows.workflowDraft.org_id ?? "");
const ownerSaving = ref(false);

const triggerKindMeta = computed(() => catalogMetadata.triggerKind(workflows.triggerDraft.kind));

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
