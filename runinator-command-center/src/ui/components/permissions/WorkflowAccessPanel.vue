<template>
  <div class="grid min-h-0 gap-3 grid-rows-[minmax(220px,0.7fr)_minmax(220px,1fr)] overflow-hidden">
    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div>
          <div class="flex items-center gap-1">
            <h3 class="m-0 text-sm font-semibold text-fg">Workflows</h3>
            <HelpBubble
              text="Select a workflow to inspect or change its user and team grants."
              label="About workflow access"
            />
          </div>
          <p class="m-0 text-xs text-fg-muted">{{ filteredWorkflows.length }} shown</p>
        </div>
        <button
          class="btn btn-primary"
          type="button"
          :disabled="!permissions.selectedWorkflowId"
          @click="openModal"
        >
          <Icon name="plus" /><span>Add Access</span>
        </button>
      </div>
      <DataTable>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Version</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="workflow in filteredWorkflows"
              :key="String(workflow.id)"
              class="cursor-pointer"
              :class="{ selected: permissions.selectedWorkflowId === workflow.id }"
              @click="permissions.selectWorkflow(String(workflow.id))"
            >
              <td>{{ workflow.name }}</td>
              <td>{{ workflow.version }}</td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </section>

    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div class="flex items-center gap-1">
          <h3 class="m-0 text-sm font-semibold text-fg">Access</h3>
          <HelpBubble
            text="View and revoke the grants applied to the selected workflow."
            label="About access grants"
          />
        </div>
        <button
          class="btn"
          type="button"
          :disabled="!permissions.selectedWorkflowId"
          @click="permissions.refreshWorkflowGrants"
        >
          <Icon name="refresh" /><span>Refresh</span>
        </button>
      </div>
      <LoadingPanel
        v-if="loading && !permissions.workflowGrants.length"
        compact
        :message="loadingMessage || 'Loading workflow access…'"
      />
      <DataTable v-else>
        <table>
          <thead>
            <tr>
              <th>Principal</th>
              <th>Type</th>
              <th>Permission</th>
              <th>Created</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="grant in permissions.workflowGrants" :key="String(grant.id)">
              <td>{{ principalLabel(grant.principal_type, grant.principal_id) }}</td>
              <td>{{ grant.principal_type }}</td>
              <td>{{ grant.permission }}</td>
              <td>{{ formatDate(grant.created_at) }}</td>
              <td>
                <button
                  class="btn btn-sm btn-ghost"
                  type="button"
                  @click="
                    revokeGrant(grant.id, principalLabel(grant.principal_type, grant.principal_id))
                  "
                >
                  Revoke
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </section>

    <div v-if="modalOpen" class="modal-backdrop" @click.self="modalOpen = false">
      <form class="modal w-full max-w-[860px]" @submit.prevent="save">
        <header class="modal-header">
          <div class="flex items-center gap-1">
            <h2>Add Workflow Access</h2>
            <HelpBubble
              text="Grant one user or team permission to view, edit, or run the selected workflow."
              label="About adding workflow access"
            />
          </div>
          <button class="btn btn-ghost" type="button" @click="modalOpen = false">
            <Icon name="x" />
          </button>
        </header>
        <div class="form-grid !grid-cols-1">
          <label
            ><span>Principal Type</span
            ><select
              v-model="permissions.grantDraft.principal_type"
              @change="permissions.grantDraft.principal_id = ''"
            >
              <option value="user">User</option>
              <option value="team">Team</option>
            </select></label
          >
          <label>
            <span>Principal</span>
            <select
              v-model="permissions.grantDraft.principal_id"
              required
              :disabled="principalOptions.length === 0"
            >
              <option value="">Principal</option>
              <option
                v-for="principal in principalOptions"
                :key="principal.id"
                :value="principal.id"
              >
                {{ principal.label }}
              </option>
            </select>
          </label>
          <label
            ><span>Permission</span
            ><select v-model="permissions.grantDraft.permission">
              <option v-for="level in permissionLevels" :key="level" :value="level">
                {{ level }}
              </option>
            </select></label
          >
        </div>
        <div class="modal-actions">
          <button class="btn" type="button" @click="modalOpen = false">Cancel</button>
          <button
            class="btn btn-primary"
            type="submit"
            :disabled="!permissions.selectedWorkflowId || !permissions.grantDraft.principal_id"
          >
            <Icon name="save" /><span>Save Access</span>
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { PrincipalType } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import { permissionLevels, usePermissionsStore } from "../../adapters/pinia/permissions";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useOperationLoading } from "../../composables/useOperationLoading";
import DataTable from "../shared/DataTable.vue";
import Icon from "../shared/Icon.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import LoadingPanel from "../shared/LoadingPanel.vue";

const app = useAppStore();
const workflows = useWorkflowsStore();
const permissions = usePermissionsStore();
const { isLoading: loading, loadingMessage } = useOperationLoading(["Loading workflow access"]);
const modalOpen = ref(false);

const filteredWorkflows = computed(() => {
  const list = workflows.workflows.filter((workflow) => workflow.id != null);

  if (!app.normalizedSearch) {
    return list;
  }

  return list.filter((workflow) =>
    [workflow.id, workflow.name, workflow.version]
      .filter(Boolean)
      .join(" ")
      .toLowerCase()
      .includes(app.normalizedSearch),
  );
});
const principalOptions = computed(() => {
  if (permissions.grantDraft.principal_type === "team") {
    return permissions.teams
      .filter((team) => team.id)
      .map((team) => ({ id: String(team.id), label: team.name }));
  }

  return permissions.users
    .filter((user) => user.id)
    .map((user) => ({ id: String(user.id), label: user.username }));
});

function selectFirstWorkflow() {
  const first = filteredWorkflows.value.at(0);

  if (first?.id) {
    void permissions.selectWorkflow(first.id);
  }
}

function openModal() {
  permissions.grantDraft.principal_id = "";
  modalOpen.value = true;
}

async function save() {
  await permissions.saveGrantDraft();

  if (!app.errorText) {
    modalOpen.value = false;
  }
}

async function revokeGrant(grantId: string | null, principal: string) {
  if (!grantId || !window.confirm(`Revoke workflow access for ${principal}?`)) {
    return;
  }

  await permissions.revokeGrant(grantId);
}

function principalLabel(type: PrincipalType, id: string) {
  if (type === "team") {
    return permissions.teams.find((team) => team.id === id)?.name ?? id;
  }

  return permissions.users.find((user) => user.id === id)?.username ?? id;
}

watch(
  filteredWorkflows,
  () => {
    if (!permissions.selectedWorkflowId) {
      selectFirstWorkflow();
    }
  },
  { immediate: true },
);
</script>
