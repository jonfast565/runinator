<template>
  <section class="pane permissions-pane">
    <div class="permissions-shell panel">
      <header class="permissions-header">
        <div>
          <h2>Permissions</h2>
          <p>Users, teams, and workflow access.</p>
        </div>
        <button class="btn" :disabled="app.loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
      </header>

      <nav class="permissions-tabs" aria-label="Permissions sections">
        <button :class="{ active: activeTab === 'users' }" type="button" @click="activeTab = 'users'">Users</button>
        <button :class="{ active: activeTab === 'teams' }" type="button" @click="activeTab = 'teams'">Teams</button>
        <button :class="{ active: activeTab === 'access' }" type="button" @click="activeTab = 'access'">Access</button>
      </nav>

      <div v-if="activeTab === 'users'" class="permissions-grid">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <h3>Users</h3>
            <span class="muted">{{ permissions.filteredUsers.length }} shown</span>
          </div>
          <DataTable>
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
                  :class="{ selected: permissions.selectedUserId === user.id, muted: user.disabled }"
                  @click="permissions.selectUser(user)"
                >
                  <td>{{ user.username }}</td>
                  <td>{{ user.email || "-" }}</td>
                  <td>{{ user.disabled ? "disabled" : "active" }}</td>
                  <td>{{ user.is_admin ? "admin" : "user" }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>

        <form class="permissions-detail" @submit.prevent="permissions.saveUserDraft">
          <div class="panel-toolbar">
            <h3>{{ permissions.selectedUser ? "Edit User" : "Create User" }}</h3>
            <div class="btn-row">
              <button class="btn" type="button" @click="permissions.clearUserDraft">
                <Icon name="x" />
                <span>New</span>
              </button>
              <button class="btn btn-danger" type="button" :disabled="!permissions.selectedUser || selectedUserIsLastEnabledAdmin" @click="confirmDeleteUser">
                <Icon name="trash" />
                <span>Delete</span>
              </button>
              <button class="btn btn-primary" type="submit">
                <Icon name="save" />
                <span>Save</span>
              </button>
            </div>
          </div>

          <div class="form-grid">
            <label>
              <span>Username</span>
              <input v-model="permissions.userDraft.username" :disabled="Boolean(permissions.selectedUser)" autocomplete="off" />
            </label>
            <label>
              <span>Email</span>
              <input v-model="permissions.userDraft.email" type="email" autocomplete="off" />
            </label>
            <label>
              <span>{{ permissions.selectedUser ? "New Password" : "Password" }}</span>
              <input v-model="permissions.userDraft.password" type="password" autocomplete="new-password" />
            </label>
            <div class="check-grid">
              <label>
                <input v-model="permissions.userDraft.is_admin" type="checkbox" :disabled="selectedUserIsLastEnabledAdmin && permissions.userDraft.is_admin" />
                <span>Admin</span>
              </label>
              <label>
                <input v-model="permissions.userDraft.disabled" type="checkbox" :disabled="selectedUserIsLastEnabledAdmin && !permissions.userDraft.disabled" />
                <span>Disabled</span>
              </label>
            </div>
          </div>

          <section class="form-section">
            <div class="section-head">
              <h4>Teams</h4>
              <div class="inline-actions">
                <select v-model="userTeamId" :disabled="!permissions.selectedUser || availableUserTeams.length === 0">
                  <option value="">Add team</option>
                  <option v-for="team in availableUserTeams" :key="String(team.id)" :value="String(team.id)">{{ team.name }}</option>
                </select>
                <button class="btn btn-sm" type="button" :disabled="!userTeamId" @click="assignUserTeam">Add</button>
              </div>
            </div>
            <div v-if="permissions.userTeams.length" class="pill-list">
              <span v-for="team in permissions.userTeams" :key="String(team.id)" class="pill">
                {{ team.name }}
                <button type="button" @click="permissions.removeSelectedUserFromTeam(String(team.id))">×</button>
              </span>
            </div>
            <div v-else class="empty-state small">No teams assigned.</div>
          </section>
        </form>
      </div>

      <div v-else-if="activeTab === 'teams'" class="permissions-grid">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <h3>Teams</h3>
            <span class="muted">{{ permissions.filteredTeams.length }} shown</span>
          </div>
          <DataTable>
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
                  :class="{ selected: permissions.selectedTeamId === team.id }"
                  @click="permissions.selectTeam(team)"
                >
                  <td>{{ team.name }}</td>
                  <td>{{ formatDate(team.created_at) }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>

        <form class="permissions-detail" @submit.prevent="permissions.saveTeamDraft">
          <div class="panel-toolbar">
            <h3>{{ permissions.selectedTeam ? "Edit Team" : "Create Team" }}</h3>
            <div class="btn-row">
              <button class="btn" type="button" @click="permissions.clearTeamDraft">
                <Icon name="x" />
                <span>New</span>
              </button>
              <button class="btn btn-danger" type="button" :disabled="!permissions.selectedTeam" @click="confirmDeleteTeam">
                <Icon name="trash" />
                <span>Delete</span>
              </button>
              <button class="btn btn-primary" type="submit">
                <Icon name="save" />
                <span>Save</span>
              </button>
            </div>
          </div>

          <div class="form-grid single">
            <label>
              <span>Name</span>
              <input v-model="permissions.teamDraftName" autocomplete="off" />
            </label>
          </div>

          <section class="form-section">
            <div class="section-head">
              <h4>Members</h4>
              <div class="inline-actions">
                <select v-model="memberUserId" :disabled="!permissions.selectedTeam || availableTeamUsers.length === 0">
                  <option value="">Add user</option>
                  <option v-for="user in availableTeamUsers" :key="String(user.id)" :value="String(user.id)">{{ user.username }}</option>
                </select>
                <button class="btn btn-sm" type="button" :disabled="!memberUserId" @click="addTeamMember">Add</button>
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
                      <button class="btn btn-sm btn-ghost" type="button" @click="permissions.removeSelectedTeamMember(String(user.id))">Remove</button>
                    </td>
                  </tr>
                </tbody>
              </table>
            </DataTable>
          </section>
        </form>
      </div>

      <div v-else class="permissions-grid access-grid">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <h3>Workflows</h3>
            <span class="muted">{{ filteredWorkflows.length }} shown</span>
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

        <section class="permissions-detail">
          <div class="panel-toolbar">
            <h3>Access</h3>
            <button class="btn" type="button" :disabled="!permissions.selectedWorkflowId" @click="permissions.refreshWorkflowGrants">
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </div>

          <form class="grant-form" @submit.prevent="permissions.saveGrantDraft">
            <select v-model="permissions.grantDraft.principal_type" @change="permissions.grantDraft.principal_id = ''">
              <option value="user">User</option>
              <option value="team">Team</option>
            </select>
            <select v-model="permissions.grantDraft.principal_id" :disabled="grantPrincipalOptions.length === 0">
              <option value="">Principal</option>
              <option v-for="principal in grantPrincipalOptions" :key="principal.id" :value="principal.id">{{ principal.label }}</option>
            </select>
            <select v-model="permissions.grantDraft.permission">
              <option v-for="level in permissionLevels" :key="level" :value="level">{{ level }}</option>
            </select>
            <button class="btn btn-primary" type="submit" :disabled="!permissions.selectedWorkflowId || !permissions.grantDraft.principal_id">
              <Icon name="save" />
              <span>Save Access</span>
            </button>
          </form>

          <DataTable>
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
                    <button class="btn btn-sm btn-ghost" type="button" @click="permissions.revokeGrant(grant.id)">Revoke</button>
                  </td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import { permissionLevels, usePermissionsStore } from "../stores/permissions";
import { useAppStore } from "../stores/app";
import { useWorkflowsStore } from "../stores/workflows";
import type { PrincipalType } from "../types/models";
import { formatDate } from "../utils/format";

const app = useAppStore();
const workflows = useWorkflowsStore();
const permissions = usePermissionsStore();
const activeTab = ref<"users" | "teams" | "access">("users");
const userTeamId = ref("");
const memberUserId = ref("");

const selectedUserIsLastEnabledAdmin = computed(() => {
  const user = permissions.selectedUser;
  return Boolean(user?.is_admin && !user.disabled && permissions.enabledAdminCount <= 1);
});

const availableUserTeams = computed(() => {
  const assigned = new Set(permissions.userTeams.map((team) => team.id));
  return permissions.teams.filter((team) => team.id && !assigned.has(team.id));
});

const availableTeamUsers = computed(() => {
  const assigned = new Set(permissions.teamMembers.map((user) => user.id));
  return permissions.users.filter((user) => user.id && !assigned.has(user.id));
});

const filteredWorkflows = computed(() => {
  const query = app.normalizedSearch;
  const list = workflows.workflows.filter((workflow) => workflow.id != null);
  if (!query) return list;
  return list.filter((workflow) => [workflow.id, workflow.name, workflow.version].filter(Boolean).join(" ").toLowerCase().includes(query));
});

const grantPrincipalOptions = computed(() => {
  if (permissions.grantDraft.principal_type === "team") {
    return permissions.teams.filter((team) => team.id).map((team) => ({ id: String(team.id), label: team.name }));
  }
  return permissions.users.filter((user) => user.id).map((user) => ({ id: String(user.id), label: user.username }));
});

async function refresh() {
  await Promise.all([
    permissions.refreshAll(),
    workflows.workflows.length === 0 ? workflows.refreshWorkflows() : Promise.resolve()
  ]);
  if (activeTab.value === "access" && !permissions.selectedWorkflowId) selectFirstWorkflow();
}

function selectFirstWorkflow() {
  const first = filteredWorkflows.value[0];
  if (first?.id) void permissions.selectWorkflow(first.id);
}

function assignUserTeam() {
  const teamId = userTeamId.value;
  userTeamId.value = "";
  void permissions.assignSelectedUserToTeam(teamId);
}

function addTeamMember() {
  const userId = memberUserId.value;
  memberUserId.value = "";
  void permissions.addSelectedTeamMember(userId);
}

function confirmDeleteUser() {
  if (!permissions.selectedUser) return;
  if (!window.confirm(`Delete user ${permissions.selectedUser.username}?`)) return;
  void permissions.deleteSelectedUser();
}

function confirmDeleteTeam() {
  if (!permissions.selectedTeam) return;
  if (!window.confirm(`Delete team ${permissions.selectedTeam.name}?`)) return;
  void permissions.deleteSelectedTeam();
}

function principalLabel(type: PrincipalType, id: string) {
  if (type === "team") return permissions.teams.find((team) => team.id === id)?.name ?? id;
  return permissions.users.find((user) => user.id === id)?.username ?? id;
}

watch(activeTab, (tab) => {
  if (tab === "access" && !permissions.selectedWorkflowId) selectFirstWorkflow();
});

onMounted(refresh);
</script>

<style scoped>
.permissions-shell {
  display: grid;
  gap: 12px;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto 1fr;
}

.permissions-header,
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.permissions-header h2,
.permissions-header p,
.permissions-detail h3,
.permissions-list h3,
.section-head h4 {
  margin: 0;
}

.permissions-header p,
.muted {
  color: var(--text-muted);
  font-size: 12px;
}

.permissions-tabs {
  display: inline-flex;
  width: fit-content;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius);
}

.permissions-tabs button {
  border: 0;
  border-right: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-muted);
  padding: 7px 12px;
}

.permissions-tabs button:last-child {
  border-right: 0;
}

.permissions-tabs button.active {
  background: var(--accent-soft);
  color: var(--text);
  font-weight: 650;
}

.permissions-grid {
  display: grid;
  min-height: 0;
  gap: 12px;
  grid-template-columns: minmax(360px, 0.9fr) minmax(420px, 1.1fr);
}

.permissions-list,
.permissions-detail {
  display: grid;
  min-width: 0;
  min-height: 0;
  gap: 12px;
  align-content: start;
  overflow: hidden;
}

.permissions-detail {
  overflow: auto;
}

.form-grid.single {
  grid-template-columns: minmax(0, 1fr);
}

.check-grid {
  display: flex;
  align-items: end;
  gap: 14px;
  min-height: 54px;
}

.check-grid label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text);
  font-size: 13px;
}

.inline-actions,
.grant-form {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.inline-actions select,
.grant-form select {
  min-width: 140px;
}

.pill-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 4px 8px;
  font-size: 12px;
}

.pill button {
  border: 0;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0;
}

.empty-state.small {
  min-height: 0;
  padding: 8px 0;
  text-align: left;
}

@media (max-width: 920px) {
  .permissions-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .permissions-shell {
    overflow: auto;
  }
}
</style>
