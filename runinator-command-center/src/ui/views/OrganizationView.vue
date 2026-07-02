<template>
  <section class="pane org-pane">
    <div class="panel">
      <div class="panel-toolbar">
        <h2>Organizations</h2>
        <button class="btn" :disabled="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
      </div>

      <div class="org-cards">
        <div class="org-card">
          <label class="org-card-label">Active organization</label>
          <select class="org-select" :value="orgs.activeOrgId ?? ''" @change="onSwitch">
            <option value="" disabled>Select an organization…</option>
            <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
              {{ m.org.name }} ({{ m.role }})
            </option>
          </select>
          <div v-if="orgs.activeOrg" class="org-card-meta">
            <span class="chip mono">org={{ orgs.activeOrg.slug }}</span>
            <span class="chip role-badge">you are {{ orgs.activeRole }}</span>
          </div>
        </div>

        <form class="org-card" @submit.prevent="createOrg">
          <label class="org-card-label">Create organization</label>
          <input v-model="newOrgName" placeholder="Acme Inc." />
          <button class="btn btn-primary" type="submit" :disabled="!newOrgName.trim()">
            <Icon name="plus" />
            <span>Create organization</span>
          </button>
        </form>
      </div>
    </div>

    <div v-if="orgs.activeOrg" class="panel">
      <div class="panel-toolbar">
        <h2>Members — {{ orgs.activeOrg.name }}</h2>
        <span class="chip role-badge">{{ members.length }} member(s)</span>
      </div>

      <div v-if="!members.length" class="empty-state">No members loaded.</div>
      <table v-else class="org-table">
        <thead>
          <tr>
            <th>User</th>
            <th>Role</th>
            <th v-if="orgs.isActiveOrgAdmin" class="col-actions"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="member in members" :key="member.user_id">
            <td>
              <span class="avatar">{{ initials(userLabel(member.user_id)) }}</span>
              <span class="member-name">{{ userLabel(member.user_id) }}</span>
            </td>
            <td>
              <select
                v-if="orgs.isActiveOrgAdmin"
                :value="member.role"
                @change="(e) => changeRole(member.user_id, e)"
              >
                <option value="member">member</option>
                <option value="admin">admin</option>
                <option value="owner">owner</option>
              </select>
              <span v-else class="chip role-badge">{{ member.role }}</span>
            </td>
            <td v-if="orgs.isActiveOrgAdmin" class="col-actions">
              <button
                class="btn btn-icon btn-ghost"
                title="Remove"
                @click="removeMember(member.user_id)"
              >
                <Icon name="trash" />
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <form v-if="orgs.isActiveOrgAdmin" class="org-inline-form" @submit.prevent="addMember">
        <input v-model="newMemberId" placeholder="User id (uuid)" />
        <select v-model="newMemberRole">
          <option value="member">member</option>
          <option value="admin">admin</option>
          <option value="owner">owner</option>
        </select>
        <button class="btn" type="submit" :disabled="!newMemberId.trim()">Add member</button>
      </form>
    </div>

    <div v-else class="panel empty-state">
      Create or select an organization to manage its members.
    </div>

    <div class="panel">
      <div class="panel-toolbar">
        <h2>Teams</h2>
        <span class="chip role-badge">{{ teams.length }} team(s)</span>
      </div>
      <p class="org-hint">
        Teams are named principals you can grant workflow access to. Add users to a team, then share
        a workflow with the whole team.
      </p>

      <div class="teams-layout">
        <div class="teams-list">
          <button
            v-for="team in teams"
            :key="team.id ?? team.name"
            type="button"
            class="team-row"
            :class="{ selected: selectedTeamId === team.id }"
            @click="selectTeam(team)"
          >
            <span class="team-name">{{ team.name }}</span>
            <Icon
              name="trash"
              class="team-delete"
              title="Delete team"
              @click.stop="removeTeam(team)"
            />
          </button>
          <form class="org-inline-form team-create" @submit.prevent="onCreateTeam">
            <input v-model="newTeamName" placeholder="New team name" />
            <button class="btn" type="submit" :disabled="!newTeamName.trim()">Create</button>
          </form>
        </div>

        <div class="team-members">
          <div v-if="!selectedTeamId" class="empty-state">Select a team to manage its members.</div>
          <template v-else>
            <h3>{{ selectedTeamName }} · members</h3>
            <div v-if="!teamMembers.length" class="empty-state">No members yet.</div>
            <ul v-else class="team-member-list">
              <li v-for="user in teamMembers" :key="user.id ?? ''">
                <span class="avatar">{{ initials(user.username) }}</span>
                <span class="member-name">{{ user.username }}</span>
                <button class="btn btn-icon btn-ghost" title="Remove" @click="removeFromTeam(user)">
                  <Icon name="trash" />
                </button>
              </li>
            </ul>
            <form class="org-inline-form" @submit.prevent="onAddTeamMember">
              <select v-model="newTeamMemberId">
                <option value="" disabled>Add a user…</option>
                <option v-for="user in users" :key="user.id ?? ''" :value="user.id ?? ''">
                  {{ user.username }}
                </option>
              </select>
              <button class="btn" type="submit" :disabled="!newTeamMemberId">Add member</button>
            </form>
          </template>
        </div>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import {
  orgAdminService,
  type OrgMembership,
  type OrgRole,
  type Team,
  type User,
} from "../../core/services";
import { useAppStore } from "../../stores/app";
import { useOrgsStore } from "../../stores/orgs";

const app = useAppStore();
const orgs = useOrgsStore();
const loading = ref(false);
const members = ref<OrgMembership[]>([]);
const users = ref<User[]>([]);
const newOrgName = ref("");
const newMemberId = ref("");
const newMemberRole = ref<OrgRole>("member");

const teams = ref<Team[]>([]);
const selectedTeamId = ref<string | null>(null);
const teamMembers = ref<User[]>([]);
const newTeamName = ref("");
const newTeamMemberId = ref("");

const selectedTeamName = computed(
  () => teams.value.find((team) => team.id === selectedTeamId.value)?.name ?? "",
);

function userLabel(userId: string): string {
  return users.value.find((u) => u.id === userId)?.username ?? userId;
}

function initials(label: string): string {
  const trimmed = label.trim();

  if (!trimmed) {
    return "?";
  }

  const parts = trimmed.split(/[\s._-]+/).filter(Boolean);
  const chars = parts.length > 1 ? `${parts[0][0]}${parts[1][0]}` : trimmed.slice(0, 2);
  return chars.toUpperCase();
}

async function refreshTeams() {
  teams.value = await orgAdminService.listTeams().catch(() => []);

  if (selectedTeamId.value && !teams.value.some((team) => team.id === selectedTeamId.value)) {
    selectedTeamId.value = null;
    teamMembers.value = [];
  }
}

async function selectTeam(team: Team) {
  selectedTeamId.value = team.id ?? null;
  newTeamMemberId.value = "";

  if (team.id) {
    teamMembers.value = await orgAdminService.listTeamMembers(team.id).catch(() => []);
  }
}

async function onCreateTeam() {
  const name = newTeamName.value.trim();

  if (!name) {
    return;
  }

  await orgAdminService.createTeam(name).catch((error: unknown) => {
      app.setError(String(error));
    });
  newTeamName.value = "";
  await refreshTeams();
}

async function removeTeam(team: Team) {
  const teamId = team.id;

  if (!teamId) {
    return;
  }

  if (!window.confirm(`Delete team "${team.name}"?`)) {
    return;
  }

  await orgAdminService.deleteTeam(teamId).catch((error: unknown) => {
      app.setError(String(error));
    });

  if (selectedTeamId.value === teamId) {
    selectedTeamId.value = null;
    teamMembers.value = [];
  }

  await refreshTeams();
}

async function onAddTeamMember() {
  const teamId = selectedTeamId.value;
  const userId = newTeamMemberId.value;

  if (!teamId || !userId) {
    return;
  }

  await orgAdminService.addTeamMember(teamId, userId).catch((error: unknown) => {
      app.setError(String(error));
    });
  newTeamMemberId.value = "";
  teamMembers.value = await orgAdminService.listTeamMembers(teamId).catch(() => []);
}

async function removeFromTeam(user: User) {
  const teamId = selectedTeamId.value;
  const userId = user.id;

  if (!teamId || !userId) {
    return;
  }

  await orgAdminService.removeTeamMember(teamId, userId).catch((error: unknown) => {
      app.setError(String(error));
    });
  teamMembers.value = await orgAdminService.listTeamMembers(teamId).catch(() => []);
}

async function refresh() {
  loading.value = true;

  try {
    await orgs.refresh();
    // resolve usernames when the caller is a platform admin; ignore a 403 otherwise.
    users.value = await orgAdminService.listUsers().catch(() => []);
    await refreshMembers();
    await refreshTeams();
  } finally {
    loading.value = false;
  }
}

async function refreshMembers() {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    members.value = [];
    return;
  }

  members.value = await orgAdminService.listMembers(orgId).catch(() => []);
}

async function refreshActiveOrgDetail() {
  members.value = [];
  teams.value = [];
  selectedTeamId.value = null;
  teamMembers.value = [];
  await Promise.all([refreshMembers(), refreshTeams()]);
}

async function onSwitch(event: Event) {
  const orgId = (event.target as HTMLSelectElement).value;

  if (orgId && (await orgs.setActive(orgId))) {
    await refreshMembers();
  }
}

async function createOrg() {
  const name = newOrgName.value.trim();

  if (!name) {
    return;
  }

  if (await orgs.create(name)) {
    newOrgName.value = "";
    await refreshMembers();
  }
}

async function addMember() {
  const orgId = orgs.activeOrgId;

  if (!orgId || !newMemberId.value.trim()) {
    return;
  }

  await orgAdminService.addMember(orgId, newMemberId.value.trim(), newMemberRole.value);
  newMemberId.value = "";
  await refreshMembers();
}

async function changeRole(userId: string, event: Event) {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    return;
  }

  const role = (event.target as HTMLSelectElement).value as OrgRole;
  await orgAdminService.updateMember(orgId, userId, role);
  await refreshMembers();
}

async function removeMember(userId: string) {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    return;
  }

  await orgAdminService.removeMember(orgId, userId);
  await refreshMembers();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refreshActiveOrgDetail);
</script>

<style scoped>
.org-pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.org-cards {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
}

.org-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 12px 14px;
}

.org-card-label {
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.org-card-meta {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.org-select {
  width: 100%;
}

.chip {
  border-radius: var(--radius-pill);
  background: var(--surface);
  color: var(--text-subtle);
  padding: 2px 8px;
  font-size: 12px;
}

.org-inline-form {
  display: flex;
  gap: 8px;
  margin-top: 12px;
  flex-wrap: wrap;
}

.org-inline-form input,
.org-inline-form select {
  flex: 1;
  min-width: 160px;
}

.org-hint {
  margin: 0 0 10px;
  color: var(--text-muted);
  font-size: 12px;
}

.org-table {
  width: 100%;
  border-collapse: collapse;
}

.org-table th,
.org-table td {
  text-align: left;
  padding: 8px 6px;
  border-bottom: 1px solid var(--border);
}

.col-actions {
  text-align: right;
  width: 1%;
}

.avatar {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  margin-right: 8px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 11px;
  font-weight: 700;
  vertical-align: middle;
}

.member-name {
  vertical-align: middle;
}

.role-badge {
  text-transform: capitalize;
}

.mono {
  font-family: var(--font-mono);
}

.teams-layout {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(200px, 260px) minmax(0, 1fr);
}

.teams-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.team-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 8px 10px;
  text-align: left;
  cursor: pointer;
}

.team-row.selected {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.team-name {
  font-weight: 600;
}

.team-delete {
  color: var(--text-muted);
  opacity: 0.7;
}

.team-delete:hover {
  color: var(--danger-fg);
  opacity: 1;
}

.team-create {
  margin-top: 4px;
}

.team-members h3 {
  margin: 0 0 8px;
}

.team-member-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.team-member-list li {
  display: flex;
  align-items: center;
  gap: 4px;
  border-bottom: 1px solid var(--border-faint);
  padding: 4px 0;
}

.team-member-list li .member-name {
  flex: 1;
}

@media (max-width: 820px) {
  .teams-layout {
    grid-template-columns: 1fr;
  }
}
</style>
