<template>
  <section class="pane users-pane">
    <div class="users-shell panel">
      <header class="users-header">
        <div>
          <h2>Users</h2>
          <p>Local accounts, team membership, and API keys.</p>
        </div>
        <div class="btn-row">
          <button class="btn" :disabled="app.loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
      </header>

      <div class="users-summary">
        <div>
          <span>Users</span>
          <strong>{{ permissions.users.length }}</strong>
        </div>
        <div>
          <span>Admins</span>
          <strong>{{ permissions.enabledAdminCount }}</strong>
        </div>
        <div>
          <span>API Keys</span>
          <strong>{{ permissions.apiKeys.length }}</strong>
        </div>
      </div>

      <div class="users-grid">
        <section class="users-list">
          <div class="panel-toolbar">
            <h3>Accounts</h3>
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
                  <td>
                    <span class="status-pill" :class="user.disabled ? 'disabled' : 'active'">
                      {{ user.disabled ? "disabled" : "active" }}
                    </span>
                  </td>
                  <td>
                    <span class="status-pill" :class="user.is_admin ? 'admin' : 'user'">
                      {{ user.is_admin ? "admin" : "user" }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </section>

        <section class="users-detail">
          <form class="detail-card" @submit.prevent="permissions.saveUserDraft">
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
          </form>

          <section class="detail-card">
            <div class="section-head">
              <h3>Teams</h3>
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
                <button type="button" @click="permissions.removeSelectedUserFromTeam(String(team.id))">x</button>
              </span>
            </div>
            <div v-else class="empty-state small">No teams assigned.</div>
          </section>

          <section class="detail-card api-key-card">
            <div class="section-head">
              <div>
                <h3>API Keys</h3>
                <p class="muted">{{ apiKeyScopeLabel }}</p>
              </div>
              <div class="btn-row">
                <button class="btn" type="button" @click="permissions.clearApiKeyDraft">
                  <Icon name="x" />
                  <span>New Key</span>
                </button>
                <button class="btn" type="button" @click="permissions.refreshApiKeys">
                  <Icon name="refresh" />
                  <span>Refresh</span>
                </button>
              </div>
            </div>

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

            <div class="api-key-grid">
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
                      :class="{ selected: permissions.selectedApiKeyId === apiKey.id, muted: apiKey.disabled }"
                      @click="permissions.selectApiKey(apiKey)"
                    >
                      <td>{{ apiKey.name }}</td>
                      <td>{{ keyOwnerLabel(apiKey) }}</td>
                      <td><code>{{ apiKey.key_prefix }}</code></td>
                      <td>{{ apiKey.disabled ? "revoked" : "active" }}</td>
                      <td>{{ apiKey.last_used_at ? formatDate(apiKey.last_used_at) : "-" }}</td>
                    </tr>
                  </tbody>
                </table>
              </DataTable>

              <form class="api-key-form" @submit.prevent="permissions.saveApiKeyDraft">
                <div class="form-grid single">
                  <label>
                    <span>Name</span>
                    <input v-model="permissions.apiKeyDraft.name" autocomplete="off" />
                  </label>
                  <label>
                    <span>Owner</span>
                    <select v-model="apiKeyOwner" :disabled="Boolean(permissions.selectedApiKey)">
                      <option value="service">Service key</option>
                      <option v-for="user in apiKeyOwnerOptions" :key="String(user.id)" :value="String(user.id)">
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
                <div class="btn-row api-key-actions">
                  <button class="btn btn-danger" type="button" :disabled="!permissions.selectedApiKey" @click="confirmRevokeApiKey">
                    <Icon name="trash" />
                    <span>Revoke</span>
                  </button>
                  <button class="btn" type="button" :disabled="!permissions.selectedApiKey || permissions.selectedApiKey.disabled" @click="confirmRotateApiKey">
                    <Icon name="refresh" />
                    <span>Rotate</span>
                  </button>
                  <button class="btn btn-primary" type="submit">
                    <Icon name="save" />
                    <span>{{ permissions.selectedApiKey ? "Save Key" : "Create Key" }}</span>
                  </button>
                </div>
              </form>
            </div>
          </section>
        </section>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import { useAppStore } from "../stores/app";
import { usePermissionsStore } from "../stores/permissions";
import type { ApiKey } from "../types/models";
import { formatDate } from "../utils/format";

const app = useAppStore();
const permissions = usePermissionsStore();
const userTeamId = ref("");

const selectedUserIsLastEnabledAdmin = computed(() => {
  const user = permissions.selectedUser;
  return Boolean(user?.is_admin && !user.disabled && permissions.enabledAdminCount <= 1);
});

const availableUserTeams = computed(() => {
  const assigned = new Set(permissions.userTeams.map((team) => team.id));
  return permissions.teams.filter((team) => team.id && !assigned.has(team.id));
});

const apiKeyOwner = computed({
  get() {
    return permissions.apiKeyDraft.is_service ? "service" : permissions.apiKeyDraft.user_id || permissions.selectedUserId || "";
  },
  set(value: string) {
    permissions.apiKeyDraft.is_service = value === "service";
    permissions.apiKeyDraft.user_id = value === "service" ? "" : value;
  }
});

const apiKeyOwnerOptions = computed(() => {
  if (permissions.selectedUser) return [permissions.selectedUser];
  return permissions.users;
});

const apiKeyScopeLabel = computed(() => {
  if (permissions.selectedUser) return `Showing service keys and keys owned by ${permissions.selectedUser.username}.`;
  return "Showing all API keys.";
});

async function refresh() {
  await permissions.refreshAll();
}

function assignUserTeam() {
  const teamId = userTeamId.value;
  userTeamId.value = "";
  void permissions.assignSelectedUserToTeam(teamId);
}

function confirmDeleteUser() {
  if (!permissions.selectedUser) return;
  if (!window.confirm(`Delete user ${permissions.selectedUser.username}?`)) return;
  void permissions.deleteSelectedUser();
}

function confirmRevokeApiKey() {
  if (!permissions.selectedApiKey) return;
  if (!window.confirm(`Revoke API key ${permissions.selectedApiKey.name}?`)) return;
  void permissions.revokeSelectedApiKey();
}

function confirmRotateApiKey() {
  if (!permissions.selectedApiKey) return;
  if (!window.confirm(`Rotate API key ${permissions.selectedApiKey.name}? The old secret will stop working.`)) return;
  void permissions.rotateSelectedApiKey();
}

async function copySecret() {
  const secret = permissions.revealedApiKey?.secret;
  if (!secret) return;
  try {
    await navigator.clipboard?.writeText(secret);
    app.setStatus("API key secret copied.");
  } catch {
    app.setError("Unable to copy API key secret.");
  }
}

function keyOwnerLabel(apiKey: ApiKey): string {
  if (apiKey.is_service) return "service";
  return permissions.users.find((user) => user.id === apiKey.user_id)?.username ?? apiKey.user_id ?? "user";
}

onMounted(refresh);
</script>

<style scoped>
.users-shell {
  display: grid;
  gap: 12px;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto 1fr;
}

.users-header,
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.users-header h2,
.users-header p,
.users-list h3,
.users-detail h3,
.section-head p {
  margin: 0;
}

.users-header p,
.muted {
  color: var(--text-muted);
  font-size: 12px;
}

.users-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.users-summary div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.users-summary span {
  color: var(--text-muted);
  font-size: 12px;
}

.users-summary strong {
  color: var(--text);
  font-size: 16px;
}

.users-grid {
  display: grid;
  min-height: 0;
  gap: 12px;
  grid-template-columns: minmax(360px, 0.9fr) minmax(480px, 1.2fr);
}

.users-list,
.users-detail {
  display: grid;
  min-width: 0;
  min-height: 0;
  gap: 12px;
  align-content: start;
  overflow: hidden;
}

.users-detail {
  overflow: auto;
}

.detail-card {
  display: grid;
  gap: 12px;
  min-width: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 12px;
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

.check-grid label,
.checkbox-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text);
  font-size: 13px;
}

.inline-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.inline-actions select {
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
  font-size: 13px;
  line-height: 1;
  padding: 0;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  border-radius: var(--radius-pill);
  padding: 2px 7px;
  font-size: 11px;
  font-weight: 650;
}

.status-pill.active,
.status-pill.admin {
  background: var(--success-bg);
  color: var(--success-fg);
}

.status-pill.disabled {
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.status-pill.user {
  background: var(--surface-subtle);
  color: var(--text-muted);
}

.api-key-card {
  min-height: 360px;
}

.api-key-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) minmax(220px, 0.52fr);
  min-height: 0;
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

.empty-state.small {
  min-height: 0;
  padding: 8px 0;
  text-align: left;
}

@media (max-width: 1100px) {
  .api-key-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

@media (max-width: 920px) {
  .users-grid,
  .users-summary {
    grid-template-columns: minmax(0, 1fr);
  }

  .users-shell {
    overflow: auto;
  }
}
</style>
