<template>
  <section class="pane h-full overflow-hidden">
    <div class="flex h-full min-h-0 flex-col gap-2.5 overflow-auto">
      <div class="panel shrink-0 gap-3">
        <header class="flex flex-wrap items-start justify-between gap-3">
          <div v-if="orgs.activeOrg" class="flex min-w-0 items-center gap-3">
            <div
              class="flex size-11 shrink-0 items-center justify-center rounded-lg border border-accent/25 bg-accent-soft text-accent-text"
            >
              <Icon name="shield" :size="21" />
            </div>
            <div class="min-w-0">
              <p class="m-0 text-[11px] font-semibold tracking-[0.08em] text-fg-muted uppercase">
                Active organization
              </p>
              <div class="mt-0.5 flex min-w-0 flex-wrap items-center gap-2">
                <h2 class="truncate text-lg font-semibold text-fg">{{ orgs.activeOrg.name }}</h2>
                <span
                  class="rounded-pill border border-border bg-surface-subtle px-2 py-0.5 text-xs font-semibold text-fg-subtle capitalize"
                >
                  {{ orgs.activeRole }}
                </span>
              </div>
              <p class="m-0 mt-0.5 text-xs text-fg-muted">
                <span class="font-mono">{{ orgs.activeOrg.slug }}</span>
                <template v-if="orgCreatedLabel"> · Created {{ orgCreatedLabel }}</template>
              </p>
            </div>
          </div>
          <div v-else>
            <p class="m-0 text-[11px] font-semibold tracking-[0.08em] text-fg-muted uppercase">
              Organization workspace
            </p>
            <h2 class="mt-0.5 text-lg font-semibold text-fg">Choose an organization</h2>
            <p class="m-0 mt-1 text-xs text-fg-muted">
              Select a workspace before managing access or teams.
            </p>
          </div>

          <div class="flex flex-wrap items-center justify-end gap-2">
            <select
              v-if="!orgs.activeOrg && orgs.memberships.length"
              aria-label="Select organization"
              class="w-auto min-w-52"
              :value="orgs.activeOrgId ?? ''"
              @change="onSwitch"
            >
              <option value="" disabled>Select an organization…</option>
              <option
                v-for="membership in orgs.memberships"
                :key="membership.org.id"
                :value="membership.org.id"
              >
                {{ membership.org.name }}
              </option>
            </select>
            <button class="btn" type="button" :disabled="loadingOrgData" @click="refresh">
              <LoadingSpinner v-if="loadingOrgData" size="sm" label="Refreshing organizations" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
            <button
              class="btn btn-primary"
              type="button"
              :aria-expanded="showCreateOrg"
              @click="showCreateOrg = !showCreateOrg"
            >
              <Icon :name="showCreateOrg ? 'x' : 'plus'" />
              <span>{{ showCreateOrg ? "Cancel" : "New organization" }}</span>
            </button>
            <button
              v-if="orgs.activeOrg && canRenameOrg"
              class="btn"
              type="button"
              :aria-expanded="showRenameOrg"
              @click="toggleRenameOrg"
            >
              <Icon :name="showRenameOrg ? 'x' : 'edit'" />
              <span>{{ showRenameOrg ? "Cancel" : "Rename" }}</span>
            </button>
          </div>
        </header>

        <form
          v-if="showCreateOrg"
          class="ui-fade-up grid gap-2 rounded-md border border-accent/25 bg-accent-soft p-3 sm:grid-cols-[minmax(220px,1fr)_auto] sm:items-end"
          @submit.prevent="createOrg"
        >
          <label class="grid gap-1 text-xs text-fg-muted">
            <span class="font-semibold text-fg">Organization name</span>
            <input
              v-model.trim="newOrgName"
              required
              minlength="2"
              maxlength="100"
              autocomplete="organization"
              placeholder="Acme Inc."
            />
            <span
              >Use the name your team recognizes. A URL-safe slug is created automatically.</span
            >
          </label>
          <button
            class="btn btn-primary sm:mb-[22px]"
            type="submit"
            :disabled="!newOrgName.trim() || creatingOrg"
          >
            <LoadingSpinner v-if="creatingOrg" size="sm" label="Creating organization" />
            <Icon v-else name="plus" />
            <span>{{ creatingOrg ? "Creating…" : "Create organization" }}</span>
          </button>
        </form>

        <form
          v-if="showRenameOrg && orgs.activeOrg"
          class="ui-fade-up grid gap-2 rounded-md border border-accent/25 bg-accent-soft p-3 sm:grid-cols-[minmax(220px,1fr)_auto] sm:items-end"
          @submit.prevent="renameOrg"
        >
          <label class="grid gap-1 text-xs text-fg-muted">
            <span class="font-semibold text-fg">Organization name</span>
            <input
              v-model.trim="renameOrgName"
              required
              minlength="2"
              maxlength="100"
              autocomplete="organization"
            />
            <span
              >Your stable slug, <code>{{ orgs.activeOrg.slug }}</code
              >, will not change.</span
            >
          </label>
          <button
            class="btn btn-primary sm:mb-[22px]"
            type="submit"
            :disabled="!renameOrgName.trim() || renamingOrg"
          >
            <LoadingSpinner v-if="renamingOrg" size="sm" label="Renaming organization" />
            <Icon v-else name="edit" />
            <span>{{ renamingOrg ? "Renaming…" : "Save name" }}</span>
          </button>
        </form>

        <EmptyState
          v-if="!orgs.activeOrg"
          compact
          icon="shield"
          title="No active organization"
          :loading="loadingOrgData"
          loading-message="Loading organizations…"
          :description="
            orgs.memberships.length
              ? 'Choose one of your organizations to continue.'
              : 'Create your first organization to add members, organize teams, and scope access.'
          "
        />

        <div v-else class="grid grid-cols-1 gap-2 sm:grid-cols-3">
          <div
            class="flex items-center gap-2.5 rounded-md border border-border-subtle bg-surface-subtle p-2.5"
          >
            <div
              class="flex size-8 items-center justify-center rounded-md bg-surface text-accent-text"
            >
              <Icon name="user" />
            </div>
            <div>
              <p class="m-0 text-xs text-fg-muted">Members</p>
              <strong class="text-base tabular-nums text-fg">{{ members.length }}</strong>
            </div>
          </div>
          <div
            class="flex items-center gap-2.5 rounded-md border border-border-subtle bg-surface-subtle p-2.5"
          >
            <div
              class="flex size-8 items-center justify-center rounded-md bg-surface text-accent-text"
            >
              <Icon name="grid" />
            </div>
            <div>
              <p class="m-0 text-xs text-fg-muted">Teams</p>
              <strong class="text-base tabular-nums text-fg">{{ teams.length }}</strong>
            </div>
          </div>
          <div
            class="flex items-center gap-2.5 rounded-md border border-border-subtle bg-surface-subtle p-2.5"
          >
            <div
              class="flex size-8 items-center justify-center rounded-md bg-surface text-accent-text"
            >
              <Icon name="shield" />
            </div>
            <div class="min-w-0">
              <p class="m-0 text-xs text-fg-muted">Your access</p>
              <strong class="block truncate text-sm text-fg capitalize">{{
                roleDescription
              }}</strong>
            </div>
          </div>
        </div>
      </div>

      <div v-if="orgs.activeOrg" class="panel min-h-[420px] shrink-0 gap-3 lg:flex-1">
        <nav
          v-if="canManageTeams"
          class="inline-flex w-fit shrink-0 overflow-hidden rounded-md border border-border"
          aria-label="Organization sections"
        >
          <button
            type="button"
            class="flex items-center gap-1.5 border-0 border-r border-border bg-surface px-3 py-1.5 text-fg-muted"
            :class="activeSection === 'members' ? 'bg-accent-soft font-semibold text-fg' : ''"
            :aria-current="activeSection === 'members' ? 'page' : undefined"
            @click="activeSection = 'members'"
          >
            <Icon name="user" />
            <span>Members</span>
            <span class="text-xs tabular-nums text-fg-muted">{{ members.length }}</span>
          </button>
          <button
            type="button"
            class="flex items-center gap-1.5 border-0 bg-surface px-3 py-1.5 text-fg-muted"
            :class="activeSection === 'teams' ? 'bg-accent-soft font-semibold text-fg' : ''"
            :aria-current="activeSection === 'teams' ? 'page' : undefined"
            @click="activeSection = 'teams'"
          >
            <Icon name="grid" />
            <span>Teams</span>
            <span class="text-xs tabular-nums text-fg-muted">{{ teams.length }}</span>
          </button>
        </nav>

        <section v-if="activeSection === 'members'" class="grid min-h-0 content-start gap-3">
          <header
            class="flex flex-wrap items-start justify-between gap-3 border-b border-border-subtle pb-2.5"
          >
            <div>
              <h2 class="text-base font-semibold text-fg">Members</h2>
              <p class="m-0 mt-0.5 text-xs text-fg-muted">
                Assign the least-privileged organization role each person needs.
              </p>
            </div>
            <div
              class="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-2 sm:flex-initial"
            >
              <label v-if="members.length > 4" class="relative min-w-44 flex-1 sm:flex-initial">
                <span class="sr-only">Search members</span>
                <input v-model.trim="memberSearch" class="w-full" placeholder="Search members" />
              </label>
              <button
                v-if="canManageMembers"
                class="btn btn-primary"
                type="button"
                :aria-expanded="showAddMember"
                @click="showAddMember = !showAddMember"
              >
                <Icon :name="showAddMember ? 'x' : 'plus'" />
                <span>{{ showAddMember ? "Cancel" : "Add member" }}</span>
              </button>
            </div>
          </header>

          <form
            v-if="canManageMembers && showAddMember"
            class="ui-fade-up grid gap-2 rounded-md border border-accent/25 bg-accent-soft p-3 md:grid-cols-[minmax(240px,1fr)_minmax(130px,180px)_auto] md:items-end"
            @submit.prevent="addMember"
          >
            <label class="grid gap-1 text-xs text-fg-muted">
              <span class="font-semibold text-fg">User</span>
              <select
                v-if="users.length"
                v-model="newMemberId"
                required
                :disabled="!availableOrgUsers.length"
              >
                <option value="" disabled>
                  {{
                    availableOrgUsers.length
                      ? "Choose a platform user…"
                      : "Every platform user is already a member"
                  }}
                </option>
                <option
                  v-for="user in availableOrgUsers"
                  :key="user.id ?? user.username"
                  :value="user.id ?? ''"
                >
                  {{ user.username }}
                </option>
              </select>
              <input
                v-else
                v-model.trim="newMemberId"
                required
                :pattern="UUID_PATTERN"
                data-validation="uuid"
                title="Enter a UUID such as 550e8400-e29b-41d4-a716-446655440000."
                placeholder="550e8400-e29b-41d4-a716-446655440000"
              />
              <span>
                {{
                  users.length
                    ? "Only users outside this organization are shown."
                    : "Enter the ID of an existing platform user."
                }}
              </span>
            </label>
            <label class="grid gap-1 text-xs text-fg-muted">
              <span class="font-semibold text-fg">Role</span>
              <select v-model="newMemberRole">
                <option value="member">Member</option>
                <option value="admin">Admin</option>
                <option value="owner">Owner</option>
              </select>
              <span>{{ newMemberRoleDescription }}</span>
            </label>
            <button class="btn btn-primary md:mb-[22px]" type="submit" :disabled="!validMemberId">
              <Icon name="plus" />
              <span>Add member</span>
            </button>
          </form>

          <LoadingPanel
            v-if="loadingOrgData && !members.length"
            compact
            :message="loadingOrgDataMessage || 'Loading members…'"
          />
          <EmptyState
            v-else-if="!members.length"
            compact
            icon="user"
            title="No members yet"
            description="Add an existing platform user to start collaborating in this organization."
          />
          <EmptyState
            v-else-if="!filteredMembers.length"
            compact
            icon="search"
            title="No matching members"
            description="Try a username, role, or user ID."
          />
          <DataTable v-else responsive="cards">
            <thead>
              <tr>
                <th>Member</th>
                <th>Role</th>
                <th v-if="canManageMembers" class="w-px"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="member in filteredMembers" :key="member.user_id">
                <td>
                  <div class="flex min-w-0 items-center gap-2.5">
                    <span
                      class="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent-soft text-[11px] font-bold text-accent-text"
                    >
                      {{ initials(userLabel(member.user_id)) }}
                    </span>
                    <span class="min-w-0">
                      <strong class="block truncate font-semibold text-fg">
                        {{ userLabel(member.user_id) }}
                      </strong>
                      <span
                        v-if="isResolvedUser(member.user_id)"
                        class="block truncate font-mono text-[11px] text-fg-muted"
                      >
                        {{ member.user_id }}
                      </span>
                    </span>
                  </div>
                </td>
                <td>
                  <select
                    v-if="canManageMembers"
                    class="w-auto min-w-32 capitalize"
                    :value="member.role"
                    :aria-label="`Role for ${userLabel(member.user_id)}`"
                    @change="(event) => changeRole(member.user_id, event)"
                  >
                    <option value="member">Member</option>
                    <option value="admin">Admin</option>
                    <option value="owner">Owner</option>
                  </select>
                  <span
                    v-else
                    class="rounded-pill border border-border bg-surface-subtle px-2 py-0.5 text-xs font-semibold text-fg-subtle capitalize"
                  >
                    {{ member.role }}
                  </span>
                </td>
                <td v-if="canManageMembers" class="w-px text-right">
                  <button
                    class="btn btn-icon btn-ghost"
                    type="button"
                    :aria-label="`Remove ${userLabel(member.user_id)}`"
                    title="Remove member"
                    @click="removeMember(member.user_id)"
                  >
                    <Icon name="trash" />
                  </button>
                </td>
              </tr>
            </tbody>
          </DataTable>
        </section>

        <section v-else class="grid min-h-0 content-start gap-3">
          <header
            class="flex flex-wrap items-start justify-between gap-3 border-b border-border-subtle pb-2.5"
          >
            <div>
              <h2 class="text-base font-semibold text-fg">Teams</h2>
              <p class="m-0 mt-0.5 text-xs text-fg-muted">
                Group organization members for reusable workflow access grants.
              </p>
            </div>
            <button
              class="btn btn-primary"
              type="button"
              :aria-expanded="showCreateTeam"
              @click="showCreateTeam = !showCreateTeam"
            >
              <Icon :name="showCreateTeam ? 'x' : 'plus'" />
              <span>{{ showCreateTeam ? "Cancel" : "New team" }}</span>
            </button>
          </header>

          <form
            v-if="showCreateTeam"
            class="ui-fade-up grid gap-2 rounded-md border border-accent/25 bg-accent-soft p-3 sm:grid-cols-[minmax(220px,1fr)_auto] sm:items-end"
            @submit.prevent="onCreateTeam"
          >
            <label class="grid gap-1 text-xs text-fg-muted">
              <span class="font-semibold text-fg">Team name</span>
              <input
                v-model.trim="newTeamName"
                required
                minlength="2"
                maxlength="100"
                placeholder="Platform engineering"
              />
              <span>Choose a name that describes the group or responsibility.</span>
            </label>
            <button
              class="btn btn-primary sm:mb-[22px]"
              type="submit"
              :disabled="!newTeamName.trim()"
            >
              <Icon name="plus" />
              <span>Create team</span>
            </button>
          </form>

          <LoadingPanel
            v-if="loadingOrgData && !teams.length"
            compact
            :message="loadingOrgDataMessage || 'Loading teams…'"
          />
          <div
            v-else
            class="grid min-h-[300px] grid-cols-1 gap-3 md:grid-cols-[minmax(220px,280px)_minmax(0,1fr)]"
          >
            <div class="flex min-w-0 flex-col gap-1.5">
              <div class="flex items-center justify-between gap-2 px-1">
                <span class="text-xs font-semibold text-fg-muted">All teams</span>
                <span class="text-xs tabular-nums text-fg-muted">{{ teams.length }}</span>
              </div>
              <EmptyState
                v-if="!teams.length"
                compact
                icon="grid"
                title="No teams yet"
                description="Create a team to group members and grant shared access."
              />
              <div
                v-for="team in teams"
                v-else
                :key="team.id ?? team.name"
                class="group flex items-center rounded-md border bg-surface transition-colors"
                :class="
                  selectedTeamId === team.id
                    ? 'border-accent bg-accent-soft'
                    : 'border-border hover:border-border-hover hover:bg-surface-hover'
                "
              >
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-2 border-0 bg-transparent px-2.5 py-2 text-left text-fg"
                  @click="selectTeam(team)"
                >
                  <span
                    class="flex size-7 shrink-0 items-center justify-center rounded-md bg-surface text-accent-text"
                  >
                    <Icon name="grid" />
                  </span>
                  <span class="min-w-0 flex-1 truncate font-semibold">{{ team.name }}</span>
                  <Icon name="chevron-right" class="shrink-0 text-fg-muted" />
                </button>
                <button
                  type="button"
                  class="btn btn-icon btn-ghost mr-1 shrink-0 text-fg-muted opacity-70 group-hover:opacity-100"
                  :aria-label="`Delete ${team.name}`"
                  title="Delete team"
                  @click="removeTeam(team)"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>

            <div class="min-w-0 rounded-md border border-border-subtle bg-surface-subtle p-3">
              <EmptyState
                v-if="!selectedTeamId"
                compact
                icon="grid"
                :title="teams.length ? 'Select a team' : 'Create your first team'"
                :description="
                  teams.length
                    ? 'Choose a team to review and manage its members.'
                    : 'Teams make shared workflow access easier to maintain.'
                "
              />
              <template v-else>
                <header
                  class="mb-3 flex flex-wrap items-start justify-between gap-2 border-b border-border-subtle pb-2.5"
                >
                  <div>
                    <p
                      class="m-0 text-[11px] font-semibold tracking-[0.08em] text-fg-muted uppercase"
                    >
                      Team
                    </p>
                    <h3 class="mt-0.5 text-base font-semibold text-fg">{{ selectedTeamName }}</h3>
                  </div>
                  <span class="rounded-pill bg-surface px-2 py-0.5 text-xs text-fg-subtle">
                    {{ teamMembers.length }}
                    {{ teamMembers.length === 1 ? "member" : "members" }}
                  </span>
                </header>
                <LoadingPanel
                  v-if="loadingTeamMembers && !teamMembers.length"
                  compact
                  :message="loadingTeamMembersMessage || 'Loading team members…'"
                />
                <EmptyState
                  v-else-if="!teamMembers.length"
                  compact
                  icon="user"
                  title="No team members yet"
                  description="Add someone who already belongs to this organization."
                />
                <ul v-else class="m-0 mb-3 flex list-none flex-col gap-1 p-0">
                  <li
                    v-for="user in teamMembers"
                    :key="user.id ?? user.username"
                    class="flex items-center gap-2 rounded-md border border-border-subtle bg-surface px-2.5 py-2"
                  >
                    <span
                      class="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-accent-soft text-[10px] font-bold text-accent-text"
                    >
                      {{ initials(user.username) }}
                    </span>
                    <span class="min-w-0 flex-1 truncate font-semibold text-fg">
                      {{ user.username }}
                    </span>
                    <button
                      class="btn btn-icon btn-ghost"
                      type="button"
                      :aria-label="`Remove ${user.username} from ${selectedTeamName}`"
                      title="Remove from team"
                      @click="removeFromTeam(user)"
                    >
                      <Icon name="trash" />
                    </button>
                  </li>
                </ul>
                <form
                  class="flex flex-wrap items-end gap-2 border-t border-border-subtle pt-3"
                  @submit.prevent="onAddTeamMember"
                >
                  <label class="grid min-w-48 flex-1 gap-1 text-xs text-fg-muted">
                    <span class="font-semibold text-fg">Add organization member</span>
                    <select
                      v-model="newTeamMemberId"
                      required
                      :disabled="!availableTeamMembers.length"
                    >
                      <option value="" disabled>
                        {{
                          availableTeamMembers.length
                            ? "Choose a member…"
                            : "Everyone is already on this team"
                        }}
                      </option>
                      <option
                        v-for="member in availableTeamMembers"
                        :key="member.user_id"
                        :value="member.user_id"
                      >
                        {{ userLabel(member.user_id) }}
                      </option>
                    </select>
                  </label>
                  <button class="btn" type="submit" :disabled="!newTeamMemberId">
                    <Icon name="plus" />
                    <span>Add to team</span>
                  </button>
                </form>
              </template>
            </div>
          </div>
        </section>
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
import {
  orgAdminService,
  type OrgMembership,
  type OrgRole,
  type Team,
  type User,
} from "../../core/services";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useCan } from "../composables/useCan";
import { useOperationLoading } from "../composables/useOperationLoading";

type OrganizationSection = "members" | "teams";

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
const { isLoading: renamingOrg } = useOperationLoading("Renaming organization");

const members = ref<OrgMembership[]>([]);
const users = ref<User[]>([]);
const teams = ref<Team[]>([]);
const teamMembers = ref<User[]>([]);
const selectedTeamId = ref<string | null>(null);
const activeSection = ref<OrganizationSection>("members");
const memberSearch = ref("");
const showCreateOrg = ref(false);
const showRenameOrg = ref(false);
const showAddMember = ref(false);
const showCreateTeam = ref(false);
const newOrgName = ref("");
const renameOrgName = ref("");
const newMemberId = ref("");
const newMemberRole = ref<OrgRole>("member");
const newTeamName = ref("");
const newTeamMemberId = ref("");

const UUID_PATTERN =
  "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}";

const canManageMembers = computed(() => can("members:manage"));
const canManageTeams = computed(() => can("members:manage"));
const canRenameOrg = computed(() => can("resource:own"));
const validMemberId = computed(() =>
  new RegExp(`^${UUID_PATTERN}$`).test(newMemberId.value.trim()),
);
const selectedTeamName = computed(
  () => teams.value.find((team) => team.id === selectedTeamId.value)?.name ?? "",
);
const orgCreatedLabel = computed(() => {
  const value = orgs.activeOrg?.created_at;

  if (!value) {
    return "";
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? ""
    : new Intl.DateTimeFormat(undefined, { month: "short", year: "numeric" }).format(date);
});
const roleDescription = computed(() => {
  switch (orgs.activeRole) {
    case "owner":
      return "Owner · full control";
    case "admin":
      return "Admin · manage access";
    default:
      return "Member · standard access";
  }
});
const newMemberRoleDescription = computed(() => {
  switch (newMemberRole.value) {
    case "owner":
      return "Full organization control";
    case "admin":
      return "Can manage members and teams";
    default:
      return "Standard organization access";
  }
});
const filteredMembers = computed(() => {
  const query = memberSearch.value.trim().toLocaleLowerCase();

  if (!query) {
    return members.value;
  }

  return members.value.filter((member) =>
    [member.user_id, member.role, userLabel(member.user_id)].some((value) =>
      value.toLocaleLowerCase().includes(query),
    ),
  );
});
const availableOrgUsers = computed(() => {
  const assigned = new Set(members.value.map((member) => member.user_id));
  return users.value.filter((user) => user.id && !assigned.has(user.id));
});
const availableTeamMembers = computed(() => {
  const assigned = new Set(teamMembers.value.map((user) => user.id).filter(Boolean));
  return members.value.filter((member) => !assigned.has(member.user_id));
});

function userLabel(userId: string): string {
  return users.value.find((user) => user.id === userId)?.username ?? userId;
}

function isResolvedUser(userId: string): boolean {
  return users.value.some((user) => user.id === userId);
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

  if (!selectedTeamId.value && teams.value[0]) {
    await selectTeam(teams.value[0]);
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

  const created = await orgAdminService.createTeam(name).catch(() => null);

  if (!created) {
    return;
  }

  newTeamName.value = "";
  showCreateTeam.value = false;
  await refreshTeams();

  const createdTeam = teams.value.find((team) => team.name === name);

  if (createdTeam) {
    await selectTeam(createdTeam);
  }
}

async function removeTeam(team: Team) {
  const teamId = team.id;

  if (!teamId || !window.confirm(`Delete team "${team.name}"?`)) {
    return;
  }

  const removed = await orgAdminService.deleteTeam(teamId).then(
    () => true,
    () => false,
  );

  if (!removed) {
    return;
  }

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

  const added = await orgAdminService.addTeamMember(teamId, userId).then(
    () => true,
    () => false,
  );

  if (!added) {
    return;
  }

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

  const removed = await orgAdminService.removeTeamMember(teamId, userId).then(
    () => true,
    () => false,
  );

  if (removed) {
    teamMembers.value = await orgAdminService.listTeamMembers(teamId).catch(() => []);
  }
}

async function refresh() {
  await orgs.refresh();
  users.value = await orgAdminService.listUsers().catch(() => []);
  await refreshActiveOrgDetail();
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
  memberSearch.value = "";
  showAddMember.value = false;
  showCreateTeam.value = false;
  showRenameOrg.value = false;
  renameOrgName.value = "";

  if (!orgs.activeOrgId) {
    return;
  }

  await Promise.all([refreshMembers(), refreshTeams()]);
}

async function onSwitch(event: Event) {
  const orgId = (event.target as HTMLSelectElement).value;

  if (orgId) {
    await orgs.setActive(orgId);
  }
}

async function createOrg() {
  const name = newOrgName.value.trim();

  if (!name) {
    return;
  }

  if (await orgs.create(name)) {
    newOrgName.value = "";
    showCreateOrg.value = false;
  }
}

function toggleRenameOrg() {
  if (showRenameOrg.value) {
    showRenameOrg.value = false;
    return;
  }

  renameOrgName.value = orgs.activeOrg?.name ?? "";
  showRenameOrg.value = true;
}

async function renameOrg() {
  const name = renameOrgName.value.trim();

  if (!name) {
    return;
  }

  if (await orgs.rename(name)) {
    showRenameOrg.value = false;
  }
}

async function addMember() {
  const orgId = orgs.activeOrgId;

  if (!orgId || !validMemberId.value) {
    return;
  }

  const added = await orgAdminService
    .addMember(orgId, newMemberId.value.trim(), newMemberRole.value)
    .then(
      () => true,
      () => false,
    );

  if (!added) {
    return;
  }

  newMemberId.value = "";
  newMemberRole.value = "member";
  showAddMember.value = false;
  await refreshMembers();
}

async function changeRole(userId: string, event: Event) {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    return;
  }

  const role = (event.target as HTMLSelectElement).value as OrgRole;
  const updated = await orgAdminService.updateMember(orgId, userId, role).then(
    () => true,
    () => false,
  );

  if (updated) {
    await refreshMembers();
  }
}

async function removeMember(userId: string) {
  const orgId = orgs.activeOrgId;

  if (!orgId || !window.confirm(`Remove ${userLabel(userId)} from this organization?`)) {
    return;
  }

  const removed = await orgAdminService.removeMember(orgId, userId).then(
    () => true,
    () => false,
  );

  if (removed) {
    await refreshMembers();
  }
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refreshActiveOrgDetail);
watch(canManageTeams, (allowed) => {
  if (!allowed) {
    activeSection.value = "members";
  }
});
</script>
