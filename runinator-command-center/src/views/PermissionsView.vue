<template>
  <section class="pane permissions-pane">
    <div class="permissions-shell panel">
      <header class="permissions-header">
        <div>
          <h2>Permissions</h2>
          <p>Users, teams, workflow access, and API keys.</p>
        </div>
        <button class="btn" :disabled="app.loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
      </header>

      <nav class="permissions-tabs" aria-label="Permissions sections">
        <button
          :class="{ active: activeTab === 'users' }"
          type="button"
          @click="activeTab = 'users'"
        >
          Users
        </button>
        <button
          :class="{ active: activeTab === 'teams' }"
          type="button"
          @click="activeTab = 'teams'"
        >
          Teams
        </button>
        <button
          :class="{ active: activeTab === 'access' }"
          type="button"
          @click="activeTab = 'access'"
        >
          Access
        </button>
        <button
          :class="{ active: activeTab === 'apiKeys' }"
          type="button"
          @click="activeTab = 'apiKeys'"
        >
          API Keys
        </button>
      </nav>

      <div v-if="activeTab === 'users'" class="permissions-content">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <div>
              <h3>Users</h3>
              <p class="muted">{{ permissions.filteredUsers.length }} shown</p>
            </div>
            <button class="btn btn-primary" type="button" @click="openNewUser">
              <Icon name="plus" />
              <span>New User</span>
            </button>
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
                  :class="{
                    selected: permissions.selectedUserId === user.id,
                    muted: user.disabled,
                  }"
                  @click="openEditUser(user)"
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
      </div>

      <div v-else-if="activeTab === 'teams'" class="permissions-content">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <div>
              <h3>Teams</h3>
              <p class="muted">{{ permissions.filteredTeams.length }} shown</p>
            </div>
            <button class="btn btn-primary" type="button" @click="openNewTeam">
              <Icon name="plus" />
              <span>New Team</span>
            </button>
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
                  @click="openEditTeam(team)"
                >
                  <td>{{ team.name }}</td>
                  <td>{{ formatDate(team.created_at) }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>
      </div>

      <div v-else-if="activeTab === 'access'" class="permissions-content access-content">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <div>
              <h3>Workflows</h3>
              <p class="muted">{{ filteredWorkflows.length }} shown</p>
            </div>
            <button
              class="btn btn-primary"
              type="button"
              :disabled="!permissions.selectedWorkflowId"
              @click="openGrantModal"
            >
              <Icon name="plus" />
              <span>Add Access</span>
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

        <section class="permissions-list">
          <div class="panel-toolbar">
            <h3>Access</h3>
            <button
              class="btn"
              type="button"
              :disabled="!permissions.selectedWorkflowId"
              @click="permissions.refreshWorkflowGrants"
            >
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </div>
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
                    <button
                      class="btn btn-sm btn-ghost"
                      type="button"
                      @click="permissions.revokeGrant(grant.id)"
                    >
                      Revoke
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>
      </div>

      <div v-else class="permissions-content">
        <section class="permissions-list">
          <div class="panel-toolbar">
            <div>
              <h3>API Keys</h3>
              <p class="muted">{{ apiKeyScopeLabel }}</p>
            </div>
            <div class="btn-row">
              <button class="btn btn-primary" type="button" @click="openNewApiKey">
                <Icon name="plus" />
                <span>New Key</span>
              </button>
              <button class="btn" type="button" @click="permissions.refreshApiKeys">
                <Icon name="refresh" />
                <span>Refresh</span>
              </button>
            </div>
          </div>

          <DataTable>
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Owner</th>
                  <th>Prefix</th>
                  <th>Status</th>
                  <th>Last Used</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="apiKey in permissions.visibleApiKeys"
                  :key="String(apiKey.id)"
                  :class="{
                    selected: permissions.selectedApiKeyId === apiKey.id,
                    muted: apiKey.disabled,
                  }"
                  @click="openEditApiKey(apiKey)"
                >
                  <td>{{ apiKey.name }}</td>
                  <td>{{ keyOwnerLabel(apiKey) }}</td>
                  <td>
                    <code>{{ apiKey.key_prefix }}</code>
                  </td>
                  <td>{{ apiKey.disabled ? "revoked" : "active" }}</td>
                  <td>{{ apiKey.last_used_at ? formatDate(apiKey.last_used_at) : "-" }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>
      </div>
    </div>

    <div v-if="userModalOpen" class="modal-backdrop" @click.self="closeUserModal">
      <form class="modal permissions-modal" @submit.prevent="saveUserModal">
        <header class="modal-header">
          <h2>{{ permissions.selectedUser ? "Edit User" : "Create User" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeUserModal">
            <Icon name="x" />
          </button>
        </header>
        <div class="form-grid">
          <label>
            <span>Username</span>
            <input
              v-model="permissions.userDraft.username"
              :disabled="Boolean(permissions.selectedUser)"
              autocomplete="off"
            />
          </label>
          <label>
            <span>Email</span>
            <input v-model="permissions.userDraft.email" type="email" autocomplete="off" />
          </label>
          <label>
            <span>{{ permissions.selectedUser ? "New Password" : "Password" }}</span>
            <input
              v-model="permissions.userDraft.password"
              type="password"
              autocomplete="new-password"
            />
          </label>
          <div class="check-grid">
            <label>
              <input
                v-model="permissions.userDraft.is_admin"
                type="checkbox"
                :disabled="selectedUserIsLastEnabledAdmin && permissions.userDraft.is_admin"
              />
              <span>Admin</span>
            </label>
            <label>
              <input
                v-model="permissions.userDraft.disabled"
                type="checkbox"
                :disabled="selectedUserIsLastEnabledAdmin && !permissions.userDraft.disabled"
              />
              <span>Disabled</span>
            </label>
          </div>
        </div>
        <section class="form-section">
          <div class="section-head">
            <h4>Teams</h4>
            <div class="inline-actions">
              <select
                v-model="userTeamId"
                :disabled="!permissions.selectedUser || availableUserTeams.length === 0"
              >
                <option value="">Add team</option>
                <option
                  v-for="team in availableUserTeams"
                  :key="String(team.id)"
                  :value="String(team.id)"
                >
                  {{ team.name }}
                </option>
              </select>
              <button
                class="btn btn-sm"
                type="button"
                :disabled="!userTeamId"
                @click="assignUserTeam"
              >
                Add
              </button>
            </div>
          </div>
          <div v-if="permissions.userTeams.length" class="pill-list">
            <span v-for="team in permissions.userTeams" :key="String(team.id)" class="pill">
              {{ team.name }}
              <button
                type="button"
                @click="permissions.removeSelectedUserFromTeam(String(team.id))"
              >
                ×
              </button>
            </span>
          </div>
          <div v-else class="empty-state small">No teams assigned.</div>
        </section>
        <div class="modal-actions">
          <button
            class="btn btn-danger"
            type="button"
            :disabled="!permissions.selectedUser || selectedUserIsLastEnabledAdmin"
            @click="confirmDeleteUser"
          >
            <Icon name="trash" />
            <span>Delete</span>
          </button>
          <button class="btn" type="button" @click="closeUserModal">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>Save</span>
          </button>
        </div>
      </form>
    </div>

    <div v-if="teamModalOpen" class="modal-backdrop" @click.self="closeTeamModal">
      <form class="modal permissions-modal" @submit.prevent="saveTeamModal">
        <header class="modal-header">
          <h2>{{ permissions.selectedTeam ? "Edit Team" : "Create Team" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeTeamModal">
            <Icon name="x" />
          </button>
        </header>
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
              <select
                v-model="memberUserId"
                :disabled="!permissions.selectedTeam || availableTeamUsers.length === 0"
              >
                <option value="">Add user</option>
                <option
                  v-for="user in availableTeamUsers"
                  :key="String(user.id)"
                  :value="String(user.id)"
                >
                  {{ user.username }}
                </option>
              </select>
              <button
                class="btn btn-sm"
                type="button"
                :disabled="!memberUserId"
                @click="addTeamMember"
              >
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
            @click="confirmDeleteTeam"
          >
            <Icon name="trash" />
            <span>Delete</span>
          </button>
          <button class="btn" type="button" @click="closeTeamModal">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>Save</span>
          </button>
        </div>
      </form>
    </div>

    <div v-if="grantModalOpen" class="modal-backdrop" @click.self="grantModalOpen = false">
      <form class="modal permissions-modal" @submit.prevent="saveGrantModal">
        <header class="modal-header">
          <h2>Add Workflow Access</h2>
          <button class="btn btn-ghost" type="button" @click="grantModalOpen = false">
            <Icon name="x" />
          </button>
        </header>
        <div class="form-grid single">
          <label>
            <span>Principal Type</span>
            <select
              v-model="permissions.grantDraft.principal_type"
              @change="permissions.grantDraft.principal_id = ''"
            >
              <option value="user">User</option>
              <option value="team">Team</option>
            </select>
          </label>
          <label>
            <span>Principal</span>
            <select
              v-model="permissions.grantDraft.principal_id"
              :disabled="grantPrincipalOptions.length === 0"
            >
              <option value="">Principal</option>
              <option
                v-for="principal in grantPrincipalOptions"
                :key="principal.id"
                :value="principal.id"
              >
                {{ principal.label }}
              </option>
            </select>
          </label>
          <label>
            <span>Permission</span>
            <select v-model="permissions.grantDraft.permission">
              <option v-for="level in permissionLevels" :key="level" :value="level">
                {{ level }}
              </option>
            </select>
          </label>
        </div>
        <div class="modal-actions">
          <button class="btn" type="button" @click="grantModalOpen = false">Cancel</button>
          <button
            class="btn btn-primary"
            type="submit"
            :disabled="!permissions.selectedWorkflowId || !permissions.grantDraft.principal_id"
          >
            <Icon name="save" />
            <span>Save Access</span>
          </button>
        </div>
      </form>
    </div>

    <div v-if="apiKeyModalOpen" class="modal-backdrop" @click.self="closeApiKeyModal">
      <form class="modal permissions-modal" @submit.prevent="saveApiKeyModal">
        <header class="modal-header">
          <h2>{{ permissions.selectedApiKey ? "Edit API Key" : "Create API Key" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeApiKeyModal">
            <Icon name="x" />
          </button>
        </header>
        <div v-if="permissions.revealedApiKey" class="secret-reveal">
          <div>
            <span>Secret for {{ permissions.revealedApiKey.api_key.name }}</span>
            <code>{{ permissions.revealedApiKey.secret }}</code>
          </div>
          <div class="btn-row">
            <button class="btn btn-sm" type="button" @click="copySecret">
              <Icon name="key" />
              <span>Copy</span>
            </button>
            <button class="btn btn-sm" type="button" @click="permissions.clearRevealedApiKey">
              <Icon name="x" />
              <span>Clear</span>
            </button>
          </div>
        </div>
        <div class="form-grid single">
          <label>
            <span>Name</span>
            <input v-model="permissions.apiKeyDraft.name" autocomplete="off" />
          </label>
          <label>
            <span>Owner</span>
            <select v-model="apiKeyOwner" :disabled="Boolean(permissions.selectedApiKey)">
              <option value="service">Service key</option>
              <option
                v-for="user in apiKeyOwnerOptions"
                :key="String(user.id)"
                :value="String(user.id)"
              >
                {{ user.username }}
              </option>
            </select>
          </label>
          <label>
            <span>Expires At</span>
            <input v-model="permissions.apiKeyDraft.expires_at" type="datetime-local" />
          </label>
          <label class="checkbox-label">
            <input v-model="permissions.apiKeyDraft.disabled" type="checkbox" />
            <span>Disabled</span>
          </label>
        </div>
        <div class="modal-actions">
          <button
            class="btn btn-danger"
            type="button"
            :disabled="!permissions.selectedApiKey"
            @click="confirmRevokeApiKey"
          >
            <Icon name="trash" />
            <span>Revoke</span>
          </button>
          <button
            class="btn"
            type="button"
            :disabled="!permissions.selectedApiKey || permissions.selectedApiKey.disabled"
            @click="confirmRotateApiKey"
          >
            <Icon name="refresh" />
            <span>Rotate</span>
          </button>
          <button class="btn" type="button" @click="closeApiKeyModal">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>{{ permissions.selectedApiKey ? "Save Key" : "Create Key" }}</span>
          </button>
        </div>
      </form>
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
import type { ApiKey, PrincipalType, Team, User } from "../types/models";
import { formatDate } from "../utils/format";

const app = useAppStore();
const workflows = useWorkflowsStore();
const permissions = usePermissionsStore();
const activeTab = ref<"users" | "teams" | "access" | "apiKeys">("users");
const userTeamId = ref("");
const memberUserId = ref("");
const userModalOpen = ref(false);
const teamModalOpen = ref(false);
const grantModalOpen = ref(false);
const apiKeyModalOpen = ref(false);

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

  if (!query) {
    return list;
  }

  return list.filter((workflow) =>
    [workflow.id, workflow.name, workflow.version]
      .filter(Boolean)
      .join(" ")
      .toLowerCase()
      .includes(query),
  );
});

const grantPrincipalOptions = computed(() => {
  if (permissions.grantDraft.principal_type === "team") {
    return permissions.teams
      .filter((team) => team.id)
      .map((team) => ({ id: String(team.id), label: team.name }));
  }

  return permissions.users
    .filter((user) => user.id)
    .map((user) => ({ id: String(user.id), label: user.username }));
});

const apiKeyOwner = computed({
  get() {
    if (permissions.apiKeyDraft.is_service) {
      return "service";
    }

    return permissions.apiKeyDraft.user_id
      ? permissions.apiKeyDraft.user_id
      : (permissions.selectedUserId ?? "");
  },
  set(value: string) {
    permissions.apiKeyDraft.is_service = value === "service";
    permissions.apiKeyDraft.user_id = value === "service" ? "" : value;
  },
});

const apiKeyOwnerOptions = computed(() => {
  if (permissions.selectedUser) {
    return [permissions.selectedUser];
  }

  return permissions.users;
});

const apiKeyScopeLabel = computed(() => {
  if (permissions.selectedUser) {
    return `Showing service keys and keys owned by ${permissions.selectedUser.username}.`;
  }

  return "Showing all API keys.";
});

async function refresh() {
  await Promise.all([
    permissions.refreshAll(),
    workflows.workflows.length === 0 ? workflows.refreshWorkflows() : Promise.resolve(),
  ]);

  if (activeTab.value === "access" && !permissions.selectedWorkflowId) {
    selectFirstWorkflow();
  }
}

function selectFirstWorkflow() {
  const first = filteredWorkflows.value[0];

  if (first.id) {
    void permissions.selectWorkflow(first.id);
  }
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

function openNewUser() {
  permissions.clearUserDraft();
  userTeamId.value = "";
  userModalOpen.value = true;
}

function openEditUser(user: User) {
  permissions.selectUser(user);
  userTeamId.value = "";
  userModalOpen.value = true;
}

function closeUserModal() {
  userModalOpen.value = false;
  userTeamId.value = "";
}

async function saveUserModal() {
  await permissions.saveUserDraft();

  if (!app.errorText) {
    closeUserModal();
  }
}

function confirmDeleteUser() {
  if (!permissions.selectedUser) {
    return;
  }

  if (!window.confirm(`Delete user ${permissions.selectedUser.username}?`)) {
    return;
  }

  void permissions.deleteSelectedUser().then(() => {
    closeUserModal();
  });
}

function openNewTeam() {
  permissions.clearTeamDraft();
  memberUserId.value = "";
  teamModalOpen.value = true;
}

function openEditTeam(team: Team) {
  permissions.selectTeam(team);
  memberUserId.value = "";
  teamModalOpen.value = true;
}

function closeTeamModal() {
  teamModalOpen.value = false;
  memberUserId.value = "";
}

async function saveTeamModal() {
  await permissions.saveTeamDraft();

  if (!app.errorText) {
    closeTeamModal();
  }
}

function confirmDeleteTeam() {
  if (!permissions.selectedTeam) {
    return;
  }

  if (!window.confirm(`Delete team ${permissions.selectedTeam.name}?`)) {
    return;
  }

  void permissions.deleteSelectedTeam().then(() => {
    closeTeamModal();
  });
}

function openGrantModal() {
  permissions.grantDraft.principal_id = "";
  grantModalOpen.value = true;
}

async function saveGrantModal() {
  await permissions.saveGrantDraft();

  if (!app.errorText) {
    grantModalOpen.value = false;
  }
}

function openNewApiKey() {
  permissions.clearApiKeyDraft();
  apiKeyModalOpen.value = true;
}

function openEditApiKey(apiKey: ApiKey) {
  permissions.selectApiKey(apiKey);
  apiKeyModalOpen.value = true;
}

function closeApiKeyModal() {
  apiKeyModalOpen.value = false;
}

function confirmRevokeApiKey() {
  if (!permissions.selectedApiKey) {
    return;
  }

  if (!window.confirm(`Revoke API key ${permissions.selectedApiKey.name}?`)) {
    return;
  }

  void permissions.revokeSelectedApiKey().then(() => {
    closeApiKeyModal();
  });
}

function confirmRotateApiKey() {
  if (!permissions.selectedApiKey) {
    return;
  }

  if (
    !window.confirm(
      `Rotate API key ${permissions.selectedApiKey.name}? The old secret will stop working.`,
    )
  ) {
    return;
  }

  void permissions.rotateSelectedApiKey();
}

async function saveApiKeyModal() {
  await permissions.saveApiKeyDraft();

  if (!app.errorText && !permissions.revealedApiKey) {
    closeApiKeyModal();
  }
}

async function copySecret() {
  const secret = permissions.revealedApiKey?.secret;

  if (!secret) {
    return;
  }

  try {
    await navigator.clipboard.writeText(secret);
    app.setStatus("API key secret copied.");
  } catch {
    app.setError("Unable to copy API key secret.");
  }
}

function principalLabel(type: PrincipalType, id: string) {
  if (type === "team") {
    return permissions.teams.find((team) => team.id === id)?.name ?? id;
  }

  return permissions.users.find((user) => user.id === id)?.username ?? id;
}

function keyOwnerLabel(apiKey: ApiKey): string {
  if (apiKey.is_service) {
    return "service";
  }

  return (
    permissions.users.find((user) => user.id === apiKey.user_id)?.username ??
    apiKey.user_id ??
    "user"
  );
}

watch(activeTab, (tab) => {
  if (tab === "access" && !permissions.selectedWorkflowId) {
    selectFirstWorkflow();
  }
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
.permissions-list h3,
.permissions-list p,
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

.permissions-content {
  min-height: 0;
  overflow: hidden;
}

.access-content {
  display: grid;
  gap: 12px;
  grid-template-rows: minmax(220px, 0.7fr) minmax(220px, 1fr);
}

.permissions-list {
  display: grid;
  min-width: 0;
  min-height: 0;
  gap: 12px;
  align-content: start;
  overflow: hidden;
}

.permissions-modal {
  width: min(860px, 100%);
}

.permissions-modal .form-section {
  display: grid;
  gap: 10px;
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

.checkbox-label {
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

.api-key-form {
  display: grid;
  align-content: start;
  gap: 12px;
  min-width: 0;
}

.api-key-actions {
  justify-content: end;
  flex-wrap: wrap;
}

.secret-reveal {
  display: flex;
  align-items: start;
  justify-content: space-between;
  gap: 12px;
  border: 1px solid var(--accent);
  border-radius: var(--radius);
  background: var(--accent-soft);
  padding: 10px 12px;
}

.secret-reveal div:first-child {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.secret-reveal span {
  color: var(--text-muted);
  font-size: 12px;
}

.secret-reveal code {
  overflow-wrap: anywhere;
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
  .permissions-shell {
    overflow: auto;
  }

  .permissions-tabs {
    max-width: 100%;
    overflow-x: auto;
  }
}
</style>
