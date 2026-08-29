<template>
  <div class="min-h-0 overflow-hidden">
    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div>
          <h3 class="m-0 text-sm font-semibold text-fg">Teams</h3>
          <p class="m-0 text-xs text-fg-muted">{{ permissions.filteredTeams.length }} shown</p>
        </div>
        <button class="btn btn-primary" type="button" @click="openNew">
          <Icon name="plus" /><span>New Team</span>
        </button>
      </div>
      <LoadingPanel
        v-if="loading && !permissions.filteredTeams.length"
        compact
        :message="loadingMessage || 'Loading teams…'"
      />
      <DataTable v-else>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="team in permissions.filteredTeams"
              :key="String(team.id)"
              class="cursor-pointer"
              :class="{ selected: permissions.selectedTeamId === team.id }"
              @click="openEdit(team)"
            >
              <td>{{ team.name }}</td>
              <td>{{ formatDate(team.created_at) }}</td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </section>

    <div v-if="modalOpen" class="modal-backdrop" @click.self="closeModal">
      <form class="modal w-full max-w-[860px]" @submit.prevent="save">
        <header class="modal-header">
          <h2>{{ permissions.selectedTeam ? "Edit Team" : "Create Team" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeModal"><Icon name="x" /></button>
        </header>
        <div class="form-grid !grid-cols-1">
          <label
            ><span>Name</span
            ><input
              v-model.trim="permissions.teamDraftName"
              required
              maxlength="100"
              autocomplete="off"
          /></label>
        </div>
        <section class="grid gap-2.5">
          <div class="flex items-center justify-between gap-3">
            <h4 class="m-0">Members</h4>
            <div class="flex min-w-0 items-center gap-1.5">
              <select
                v-model="memberId"
                class="min-w-[140px]"
                :disabled="!permissions.selectedTeam || availableUsers.length === 0"
              >
                <option value="">Add user</option>
                <option
                  v-for="user in availableUsers"
                  :key="String(user.id)"
                  :value="String(user.id)"
                >
                  {{ user.username }}
                </option>
              </select>
              <button class="btn btn-sm" type="button" :disabled="!memberId" @click="addMember">
                Add
              </button>
            </div>
          </div>
          <DataTable>
            <table class="compact">
              <thead>
                <tr>
                  <th>Username</th>
                  <th>Email</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="user in permissions.teamMembers" :key="String(user.id)">
                  <td>{{ user.username }}</td>
                  <td>{{ user.email || "-" }}</td>
                  <td>
                    <button
                      class="btn btn-sm btn-ghost"
                      type="button"
                      @click="permissions.removeSelectedTeamMember(String(user.id))"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>
        <div class="modal-actions">
          <button
            class="btn btn-danger"
            type="button"
            :disabled="!permissions.selectedTeam"
            @click="confirmDelete"
          >
            <Icon name="trash" /><span>Delete</span>
          </button>
          <button class="btn" type="button" @click="closeModal">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" /><span>Save</span>
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { Team } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import { usePermissionsStore } from "../../adapters/pinia/permissions";
import { useOperationLoading } from "../../composables/useOperationLoading";
import DataTable from "../shared/DataTable.vue";
import Icon from "../shared/Icon.vue";
import LoadingPanel from "../shared/LoadingPanel.vue";

const app = useAppStore();
const permissions = usePermissionsStore();
const { isLoading: loading, loadingMessage } = useOperationLoading(["Loading permissions"]);
const modalOpen = ref(false);
const memberId = ref("");
const availableUsers = computed(() => {
  const assigned = new Set(permissions.teamMembers.map((user) => user.id));
  return permissions.users.filter((user) => user.id && !assigned.has(user.id));
});

function openNew() {
  permissions.clearTeamDraft();
  memberId.value = "";
  modalOpen.value = true;
}

function openEdit(team: Team) {
  permissions.selectTeam(team);
  memberId.value = "";
  modalOpen.value = true;
}

function closeModal() {
  modalOpen.value = false;
  memberId.value = "";
}

function addMember() {
  const selected = memberId.value;
  memberId.value = "";
  void permissions.addSelectedTeamMember(selected);
}

async function save() {
  await permissions.saveTeamDraft();

  if (!app.errorText) {
    closeModal();
  }
}

function confirmDelete() {
  const team = permissions.selectedTeam;

  if (!team || !window.confirm(`Delete team ${team.name}?`)) {
    return;
  }

  void permissions.deleteSelectedTeam().then(closeModal);
}
</script>
