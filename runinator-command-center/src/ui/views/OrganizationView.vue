<template>
  <section class="pane h-full overflow-hidden">
    <div class="flex h-full min-h-0 flex-col gap-2.5 overflow-auto">
      <div class="panel shrink-0">
        <PanelHeader
          title="Organizations"
          description="Select the organization you intend to change before managing members or teams."
        >
          <button class="btn" :disabled="loadingOrgData" @click="refresh">
            <LoadingSpinner v-if="loadingOrgData" size="sm" label="Refreshing organizations" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </PanelHeader>

        <div class="grid grid-cols-[repeat(auto-fit,minmax(260px,1fr))] gap-3">
          <div
            class="flex flex-col gap-2 rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3"
          >
            <label class="text-xs tracking-wide text-fg-muted uppercase">Active organization</label>
            <select class="w-full" :value="orgs.activeOrgId ?? ''" @change="onSwitch">
              <option value="" disabled>Select an organization…</option>
              <option v-for="m in orgs.memberships" :key="m.org.id" :value="m.org.id">
                {{ m.org.name }} ({{ m.role }})
              </option>
            </select>
            <div v-if="orgs.activeOrg" class="flex flex-wrap gap-2">
              <span class="rounded-pill bg-surface px-2 py-0.5 font-mono text-xs text-fg-subtle"
                >org={{ orgs.activeOrg.slug }}</span
              >
              <span class="rounded-pill bg-surface px-2 py-0.5 text-xs text-fg-subtle capitalize"
                >you are {{ orgs.activeRole }}</span
              >
            </div>
          </div>

          <form
            class="flex flex-col gap-2 rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3"
            @submit.prevent="createOrg"
          >
            <label class="text-xs tracking-wide text-fg-muted uppercase">Create organization</label>
            <input
              v-model.trim="newOrgName"
              required
              minlength="2"
              maxlength="100"
              placeholder="Acme Inc."
            />
            <button
              class="btn btn-primary"
              type="submit"
              :disabled="!newOrgName.trim() || creatingOrg"
            >
              <LoadingSpinner v-if="creatingOrg" size="sm" label="Creating organization" />
              <Icon v-else name="plus" />
              <span>{{ creatingOrg ? "Creating…" : "Create organization" }}</span>
            </button>
          </form>
        </div>
      </div>

      <div class="panel shrink-0">
        <PanelHeader
          description="Add existing users to the active organization and assign the least-privileged role they need."
        >
          <template #title>
            Members<template v-if="orgs.activeOrg"> — {{ orgs.activeOrg.name }}</template>
          </template>
          <span
            v-if="orgs.activeOrg"
            class="rounded-pill bg-surface px-2 py-0.5 text-xs text-fg-subtle"
            >{{ members.length }} member(s)</span
          >
        </PanelHeader>

        <EmptyState
          v-if="!orgs.activeOrg"
          icon="shield"
          title="No organization selected"
          :loading="loadingOrgData"
          loading-message="Loading organizations…"
          description="Choose an organization above before adding members or teams."
        />
        <LoadingPanel
          v-else-if="loadingOrgData && !members.length"
          compact
          :message="loadingOrgDataMessage || 'Loading members…'"
        />
        <EmptyState
          v-else-if="!members.length"
          compact
          icon="shield"
          title="No members yet"
          description="Add an existing user's UUID below and start with the member role unless they need administration access."
        />
        <DataTable v-else>
          <table>
            <thead>
              <tr>
                <th>User</th>
                <th>Role</th>
                <th v-if="can('org:members:manage')" class="w-px"></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="member in members" :key="member.user_id">
                <td>
                  <span
                    class="mr-2 inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent-soft align-middle text-[11px] font-bold text-accent"
                    >{{ initials(userLabel(member.user_id)) }}</span
                  >
                  <span class="align-middle">{{ userLabel(member.user_id) }}</span>
                </td>
                <td>
                  <select
                    v-if="can('org:members:manage')"
                    class="w-auto min-w-32"
                    :value="member.role"
                    @change="(e) => changeRole(member.user_id, e)"
                  >
                    <option value="member">member</option>
                    <option value="admin">admin</option>
                    <option value="owner">owner</option>
                  </select>
                  <span
                    v-else
                    class="rounded-pill bg-surface px-2 py-0.5 text-xs text-fg-subtle capitalize"
                    >{{ member.role }}</span
                  >
                </td>
                <td v-if="can('org:members:manage')" class="w-px text-right">
                  <button
                    class="btn btn-icon btn-ghost"
                    title="Remove member"
                    @click="removeMember(member.user_id)"
                  >
                    <Icon name="trash" />
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </DataTable>

        <form
          v-if="orgs.activeOrg && can('org:members:manage')"
          class="flex flex-wrap gap-2"
          @submit.prevent="addMember"
        >
          <input
            v-model.trim="newMemberId"
            class="min-w-40 flex-1"
            required
            :pattern="UUID_PATTERN"
            title="Enter a UUID such as 550e8400-e29b-41d4-a716-446655440000."
            placeholder="User id (uuid)"
          />
          <select v-model="newMemberRole" class="w-auto min-w-32">
            <option value="member">member</option>
            <option value="admin">admin</option>
            <option value="owner">owner</option>
          </select>
          <button class="btn" type="submit" :disabled="!validMemberId">Add member</button>
        </form>
      </div>

      <div v-if="can('teams:manage')" class="panel shrink-0">
        <PanelHeader
          title="Teams"
          description="Group organization members into teams for reusable workflow access grants."
        >
          <span class="rounded-pill bg-surface px-2 py-0.5 text-xs text-fg-subtle"
            >{{ teams.length }} team(s)</span
          >
        </PanelHeader>

        <div class="grid grid-cols-1 gap-3 md:grid-cols-[minmax(200px,260px)_minmax(0,1fr)]">
          <div class="flex flex-col gap-1.5">
            <LoadingPanel
              v-if="loadingOrgData && !teams.length"
              compact
              :message="loadingOrgDataMessage || 'Loading teams…'"
            />
            <button
              v-for="team in teams"
              :key="team.id ?? team.name"
              type="button"
              class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-md border border-border bg-surface px-2.5 py-2 text-left"
              :class="selectedTeamId === team.id ? 'border-accent bg-accent-soft' : ''"
              @click="selectTeam(team)"
            >
              <span class="min-w-0 truncate font-semibold">{{ team.name }}</span>
              <Icon
                name="trash"
                class="shrink-0 text-fg-muted opacity-70 hover:text-danger-fg hover:opacity-100"
                title="Delete team"
                @click.stop="removeTeam(team)"
              />
            </button>
            <form class="mt-1 flex flex-wrap gap-2" @submit.prevent="onCreateTeam">
              <input
                v-model.trim="newTeamName"
                class="min-w-40 flex-1"
                required
                minlength="2"
                maxlength="100"
                placeholder="New team name"
              />
              <button class="btn" type="submit" :disabled="!newTeamName.trim()">Create</button>
            </form>
          </div>

          <div class="min-w-0 rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3">
            <EmptyState
              v-if="!selectedTeamId"
              compact
              icon="list"
              :title="teams.length ? 'Pick a team' : 'No teams yet'"
              :description="
                teams.length
                  ? 'Select a team to review its members.'
                  : 'Create a team to group users before assigning shared permissions.'
              "
            />
            <template v-else>
              <h3 class="m-0 mb-2 text-sm font-semibold text-fg">
                {{ selectedTeamName }} · members
              </h3>
              <LoadingPanel
                v-if="loadingTeamMembers && !teamMembers.length"
                compact
                :message="loadingTeamMembersMessage || 'Loading team members…'"
              />
              <EmptyState
                v-else-if="!teamMembers.length"
                compact
                icon="shield"
                title="No members yet"
                description="Add an organization member below; users must belong to the organization before joining a team."
              />
              <ul v-else class="m-0 flex list-none flex-col gap-1 p-0">
                <li
                  v-for="user in teamMembers"
                  :key="user.id ?? ''"
                  class="flex items-center gap-1 border-b border-border-faint py-1"
                >
                  <span
                    class="mr-2 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent-soft text-[11px] font-bold text-accent"
                    >{{ initials(user.username) }}</span
                  >
                  <span class="min-w-0 flex-1 truncate">{{ user.username }}</span>
                  <button
                    class="btn btn-icon btn-ghost"
                    title="Remove from team"
                    @click="removeFromTeam(user)"
                  >
                    <Icon name="trash" />
                  </button>
                </li>
              </ul>
              <form class="mt-3 flex flex-wrap gap-2" @submit.prevent="onAddTeamMember">
                <select v-model="newTeamMemberId" class="min-w-40 flex-1">
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
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import {
  orgAdminService,
  type OrgMembership,
  type OrgRole,
  type Team,
  type User,
} from "../../core/services";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useCan } from "../composables/useCan";
import { useOperationLoading } from "../composables/useOperationLoading";

const app = useAppStore();
const orgs = useOrgsStore();
const { can } = useCan();
const { isLoading: loadingOrgData, loadingMessage: loadingOrgDataMessage } = useOperationLoading([
  "Loading organizations",
  "Loading org members",
  "Loading users",
  "Loading teams",
]);
const { isLoading: loadingTeamMembers, loadingMessage: loadingTeamMembersMessage } =
  useOperationLoading("Loading team members");
const { isLoading: creatingOrg } = useOperationLoading("Creating organization");
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
const UUID_PATTERN =
  "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}";
const validMemberId = computed(() =>
  new RegExp(`^${UUID_PATTERN}$`).test(newMemberId.value.trim()),
);

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

  if (
    !teamId ||
    !userId ||
    !window.confirm(`Remove ${user.username} from ${selectedTeamName.value}?`)
  ) {
    return;
  }

  await orgAdminService.removeTeamMember(teamId, userId).catch((error: unknown) => {
    app.setError(String(error));
  });
  teamMembers.value = await orgAdminService.listTeamMembers(teamId).catch(() => []);
}

async function refresh() {
  await orgs.refresh();
  // resolve usernames when the caller is a platform admin; ignore a 403 otherwise.
  users.value = await orgAdminService.listUsers().catch(() => []);
  await refreshMembers();
  await refreshTeams();
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

  if (!orgId || !validMemberId.value) {
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

  if (!orgId || !window.confirm(`Remove ${userLabel(userId)} from this organization?`)) {
    return;
  }

  await orgAdminService.removeMember(orgId, userId);
  await refreshMembers();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refreshActiveOrgDetail);
</script>
