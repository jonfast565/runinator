<template>
  <div class="min-h-0 overflow-hidden">
    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div>
          <h3 class="m-0 text-sm font-semibold text-fg">Users</h3>
          <p class="m-0 text-xs text-fg-muted">{{ permissions.filteredUsers.length }} shown</p>
        </div>
        <button class="btn btn-primary" type="button" @click="openNewUser">
          <Icon name="plus" />
          <span>New User</span>
        </button>
      </div>
      <LoadingPanel
        v-if="loading && !permissions.filteredUsers.length"
        compact
        :message="loadingMessage || 'Loading users…'"
      />
      <DataTable v-else>
        <table>
          <thead>
            <tr>
              <th>Username</th>
              <th>Email</th>
              <th>Status</th>
              <th>Role</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="user in permissions.filteredUsers"
              :key="String(user.id)"
              class="cursor-pointer"
              :class="{ selected: permissions.selectedUserId === user.id, muted: user.disabled }"
              @click="openEditUser(user)"
            >
              <td>{{ user.username }}</td>
              <td>{{ user.email || "-" }}</td>
              <td>{{ user.disabled ? "disabled" : "active" }}</td>
              <td>{{ user.platform_role }}</td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </section>

    <div v-if="modalOpen" class="modal-backdrop" @click.self="closeModal">
      <form class="modal w-full max-w-[860px]" @submit.prevent="save">
        <header class="modal-header">
          <h2>{{ permissions.selectedUser ? "Edit User" : "Create User" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeModal"><Icon name="x" /></button>
        </header>
        <div class="form-grid">
          <label>
            <span>Username</span>
            <input
              v-model.trim="permissions.userDraft.username"
              required
              maxlength="100"
              :disabled="Boolean(permissions.selectedUser)"
              autocomplete="off"
            />
          </label>
          <label
            ><span>Email</span
            ><input v-model="permissions.userDraft.email" type="email" autocomplete="off"
          /></label>
          <label>
            <span>{{ permissions.selectedUser ? "New Password" : "Password" }}</span>
            <input
              v-model="permissions.userDraft.password"
              type="password"
              :required="!permissions.selectedUser"
              autocomplete="new-password"
            />
          </label>
          <div class="flex min-h-[54px] items-end gap-3.5">
            <label
              ><span>Platform role</span
              ><select v-model="permissions.userDraft.platform_role" :disabled="isLastEnabledAdmin">
                <option value="member">Member</option>
                <option value="auditor">Auditor</option>
                <option value="operator">Operator</option>
                <option value="admin">Admin</option>
              </select></label
            >
            <label class="inline-flex items-center gap-1.5 text-[13px] text-fg">
              <input
                v-model="permissions.userDraft.disabled"
                type="checkbox"
                :disabled="isLastEnabledAdmin && !permissions.userDraft.disabled"
              />
              <span>Disabled</span>
            </label>
          </div>
        </div>
        <section class="grid gap-2.5">
          <div class="flex items-center justify-between gap-3">
            <h4 class="m-0">Teams</h4>
            <div class="flex min-w-0 items-center gap-1.5">
              <select
                v-model="teamId"
                class="min-w-[140px]"
                :disabled="!permissions.selectedUser || availableTeams.length === 0"
              >
                <option value="">Add team</option>
                <option
                  v-for="team in availableTeams"
                  :key="String(team.id)"
                  :value="String(team.id)"
                >
                  {{ team.name }}
                </option>
              </select>
              <button class="btn btn-sm" type="button" :disabled="!teamId" @click="assignTeam">
                Add
              </button>
            </div>
          </div>
          <div v-if="permissions.userTeams.length" class="flex flex-wrap gap-1.5">
            <span
              v-for="team in permissions.userTeams"
              :key="String(team.id)"
              class="inline-flex items-center gap-1.5 rounded-pill border border-border bg-surface-subtle px-2 py-1 text-xs"
            >
              {{ team.name }}
              <button
                type="button"
                class="cursor-pointer border-0 bg-transparent p-0 text-sm leading-none text-fg-muted"
                @click="permissions.removeSelectedUserFromTeam(String(team.id))"
              >
                ×
              </button>
            </span>
          </div>
          <div v-else class="min-h-0 py-2 text-left text-fg-muted">No teams assigned.</div>
        </section>
        <div class="modal-actions">
          <button
            class="btn btn-danger"
            type="button"
            :disabled="!permissions.selectedUser || isLastEnabledAdmin"
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
import type { User } from "../../../core/domain/models";
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
const teamId = ref("");

const isLastEnabledAdmin = computed(() => {
  const user = permissions.selectedUser;
  return user?.platform_role === "admin" && !user.disabled && permissions.enabledAdminCount <= 1;
});
const availableTeams = computed(() => {
  const assigned = new Set(permissions.userTeams.map((team) => team.id));
  return permissions.teams.filter((team) => team.id && !assigned.has(team.id));
});

function openNewUser() {
  permissions.clearUserDraft();
  teamId.value = "";
  modalOpen.value = true;
}

function openEditUser(user: User) {
  permissions.selectUser(user);
  teamId.value = "";
  modalOpen.value = true;
}

function closeModal() {
  modalOpen.value = false;
  teamId.value = "";
}

function assignTeam() {
  const selected = teamId.value;
  teamId.value = "";
  void permissions.assignSelectedUserToTeam(selected);
}

async function save() {
  await permissions.saveUserDraft();

  if (!app.errorText) {
    closeModal();
  }
}

function confirmDelete() {
  const user = permissions.selectedUser;

  if (!user || !window.confirm(`Delete user ${user.username}?`)) {
    return;
  }

  void permissions.deleteSelectedUser().then(closeModal);
}
</script>
