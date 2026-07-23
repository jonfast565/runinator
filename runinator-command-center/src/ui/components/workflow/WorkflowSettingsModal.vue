<template>
  <div class="modal-backdrop">
    <form class="modal w-[min(1040px,100%)]" @submit.prevent="workflows.closeWorkflowSettings">
      <header class="modal-header">
        <h2>Workflow Settings</h2>
        <button type="button" @click="workflows.closeWorkflowSettings">Close</button>
      </header>

      <section class="form-section">
        <div class="form-grid">
          <label
            >Name
            <input v-model="workflows.workflowDraft.name" @input="workflows.markWorkflowDirty"
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
          <label
            >Concurrency
            <input
              v-model.number="workflows.workflowConcurrency"
              type="number"
              min="1"
              max="256"
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
      </section>

      <section class="form-section ownership-section">
        <div class="section-toolbar">
          <h3>Ownership</h3>
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
          <p class="hint">
            Scoping a workflow to an org limits its runs and visibility to that org's members. Share
            with individual users or teams from the Share dialog. Only org admins can move a
            workflow into an org.
          </p>
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
          <table class="compact">
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
          </table>
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
              <p v-else-if="catalogMetadata.loading || !catalogMetadata.loaded" class="hint catalog-loading-hint">
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
        <button type="submit">Done</button>
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
import JsonEditor from "../shared/JsonEditor.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import CatalogFieldEditor from "./CatalogFieldEditor.vue";

const workflows = useWorkflowsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const catalogMetadata = useCatalogMetadataStore();

const ownerOrgId = ref<string>(workflows.workflowDraft.org_id ?? "");
const ownerSaving = ref(false);

const triggerKindMeta = computed(() =>
  workflows.triggerDraft.kind ? catalogMetadata.triggerKind(workflows.triggerDraft.kind) : null,
);

// local mutable config record kept in sync with the trigger json for field-by-field editing.
const configDraft = ref<Record<string, unknown>>({});

watch(
  () => workflows.triggerJson.configuration,
  (json) => {
    try {
      const parsed = JSON.parse(json) as unknown;
      configDraft.value =
        parsed && typeof parsed === "object" && !Array.isArray(parsed)
          ? (parsed as Record<string, unknown>)
          : {};
    } catch {
      configDraft.value = {};
    }
  },
  { immediate: true },
);

function setConfigField(name: string, value: unknown) {
  configDraft.value = { ...configDraft.value, [name]: value };
  workflows.triggerJson.configuration = JSON.stringify(configDraft.value, null, 2);
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

if (!orgs.memberships.length) {
  void orgs.refresh();
}
</script>
