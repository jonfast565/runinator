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

      <div class="org-switcher">
        <label>Active organization</label>
        <div class="org-switcher-row">
          <select :value="orgs.activeOrgId ?? ''" @change="onSwitch">
            <option value="" disabled>Select an organization…</option>
            <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
              {{ m.org.name }} ({{ m.role }})
            </option>
          </select>
          <span v-if="orgs.activeOrg" class="org-slug mono">org={{ orgs.activeOrg.slug }}</span>
        </div>
      </div>

      <form class="org-create" @submit.prevent="createOrg">
        <input v-model="newOrgName" placeholder="New organization name" />
        <button class="btn primary" type="submit" :disabled="!newOrgName.trim()">
          <Icon name="add" />
          <span>Create</span>
        </button>
      </form>
    </div>

    <div v-if="orgs.activeOrg" class="panel">
      <div class="panel-toolbar">
        <h2>Members — {{ orgs.activeOrg.name }}</h2>
        <span class="role-badge">{{ orgs.activeRole }}</span>
      </div>

      <div v-if="!members.length" class="empty-state">No members loaded.</div>
      <table v-else class="org-table">
        <thead>
          <tr><th>User</th><th>Role</th><th v-if="orgs.isActiveOrgAdmin"></th></tr>
        </thead>
        <tbody>
          <tr v-for="member in members" :key="member.user_id">
            <td class="mono">{{ userLabel(member.user_id) }}</td>
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
              <span v-else class="role-badge">{{ member.role }}</span>
            </td>
            <td v-if="orgs.isActiveOrgAdmin">
              <button class="btn danger small" @click="removeMember(member.user_id)">Remove</button>
            </td>
          </tr>
        </tbody>
      </table>

      <form v-if="orgs.isActiveOrgAdmin" class="org-add-member" @submit.prevent="addMember">
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
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import Icon from "../components/shared/Icon.vue";
import {
  addOrgMember,
  listOrgMembers,
  listUsers,
  removeOrgMember,
  updateOrgMember,
  type OrgMembership,
  type OrgRole
} from "../api/commandCenterApi";
import type { User } from "../types/models";
import { useAppStore } from "../stores/app";
import { useOrgsStore } from "../stores/orgs";

const app = useAppStore();
const orgs = useOrgsStore();
const loading = ref(false);
const members = ref<OrgMembership[]>([]);
const users = ref<User[]>([]);
const newOrgName = ref("");
const newMemberId = ref("");
const newMemberRole = ref<OrgRole>("member");

function userLabel(userId: string): string {
  return users.value.find((u) => u.id === userId)?.username ?? userId;
}

async function refresh() {
  loading.value = true;
  try {
    await orgs.refresh();
    // resolve usernames when the caller is a platform admin; ignore a 403 otherwise.
    users.value = await listUsers().catch(() => []);
    await refreshMembers();
  } finally {
    loading.value = false;
  }
}

async function refreshMembers() {
  if (!orgs.activeOrgId) {
    members.value = [];
    return;
  }
  members.value = await app
    .runOperation("Loading members", () => listOrgMembers(orgs.activeOrgId as string))
    .catch(() => []);
}

async function onSwitch(event: Event) {
  const orgId = (event.target as HTMLSelectElement).value;
  if (orgId && (await orgs.setActive(orgId))) await refreshMembers();
}

async function createOrg() {
  const name = newOrgName.value.trim();
  if (!name) return;
  if (await orgs.create(name)) {
    newOrgName.value = "";
    await refreshMembers();
  }
}

async function addMember() {
  const orgId = orgs.activeOrgId;
  if (!orgId || !newMemberId.value.trim()) return;
  await app.runOperation("Adding member", () =>
    addOrgMember(orgId, newMemberId.value.trim(), newMemberRole.value)
  );
  newMemberId.value = "";
  await refreshMembers();
}

async function changeRole(userId: string, event: Event) {
  const orgId = orgs.activeOrgId;
  if (!orgId) return;
  const role = (event.target as HTMLSelectElement).value as OrgRole;
  await app.runOperation("Updating role", () => updateOrgMember(orgId, userId, role));
  await refreshMembers();
}

async function removeMember(userId: string) {
  const orgId = orgs.activeOrgId;
  if (!orgId) return;
  await app.runOperation("Removing member", () => removeOrgMember(orgId, userId));
  await refreshMembers();
}

onMounted(refresh);
</script>

<style scoped>
.org-pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.org-switcher {
  margin-bottom: 12px;
}

.org-switcher label {
  display: block;
  margin-bottom: 4px;
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.org-switcher-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.org-slug {
  color: var(--text-muted);
  font-size: 12px;
}

.org-create,
.org-add-member {
  display: flex;
  gap: 8px;
  margin-top: 12px;
  flex-wrap: wrap;
}

.org-create input,
.org-add-member input {
  flex: 1;
  min-width: 180px;
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

.role-badge {
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  color: var(--text-subtle);
  padding: 2px 8px;
  font-size: 12px;
  text-transform: capitalize;
}

.mono {
  font-family: var(--font-mono);
}

.btn.small {
  padding: 2px 8px;
  font-size: 12px;
}

.btn.danger {
  color: var(--danger-fg);
}
</style>
