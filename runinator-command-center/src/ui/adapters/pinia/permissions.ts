import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  addTeamMember,
  createApiKey,
  createTeam,
  createUser,
  deleteTeam,
  deleteUser,
  grantWorkflowAccess,
  listApiKeys,
  listTeamMembers,
  listTeams,
  listUserTeams,
  listUsers,
  listWorkflowGrants,
  removeTeamMember,
  revokeApiKey,
  revokeWorkflowGrant,
  rotateApiKey,
  updateApiKey,
  updateTeam,
  updateUser,
  type CreateApiKeyInput,
  type CreateUserInput,
  type UpdateApiKeyInput,
  type UpdateUserInput,
} from "../../../api/commandCenterApi";
import type {
  ApiKey,
  CreateApiKeyResponse,
  Grant,
  PermissionLevel,
  PrincipalType,
  Team,
  User,
} from "../../../types/models";
import { useAppStore } from "./app";

export const permissionLevels: PermissionLevel[] = ["view", "run", "edit", "own"];

export interface UserDraft {
  username: string;
  email: string;
  password: string;
  is_admin: boolean;
  disabled: boolean;
}

export interface GrantDraft {
  principal_type: PrincipalType;
  principal_id: string;
  permission: PermissionLevel;
}

export interface ApiKeyDraft {
  name: string;
  user_id: string;
  is_service: boolean;
  expires_at: string;
  disabled: boolean;
}

function blankUserDraft(): UserDraft {
  return {
    username: "",
    email: "",
    password: "",
    is_admin: false,
    disabled: false,
  };
}

function userDraftFrom(user: User): UserDraft {
  return {
    username: user.username,
    email: user.email ?? "",
    password: "",
    is_admin: user.is_admin,
    disabled: user.disabled,
  };
}

function blankApiKeyDraft(userId: string | null = null): ApiKeyDraft {
  return {
    name: "",
    user_id: userId ?? "",
    is_service: false,
    expires_at: "",
    disabled: false,
  };
}

function apiKeyDraftFrom(apiKey: ApiKey): ApiKeyDraft {
  return {
    name: apiKey.name,
    user_id: apiKey.user_id ?? "",
    is_service: apiKey.is_service,
    expires_at: apiKey.expires_at ? toDateTimeInput(apiKey.expires_at) : "",
    disabled: apiKey.disabled,
  };
}

export const usePermissionsStore = defineStore("permissions", () => {
  const app = useAppStore();
  const users = ref<User[]>([]);
  const teams = ref<Team[]>([]);
  const apiKeys = ref<ApiKey[]>([]);
  const selectedUserId = ref<string | null>(null);
  const selectedTeamId = ref<string | null>(null);
  const selectedApiKeyId = ref<string | null>(null);
  const selectedWorkflowId = ref<string | null>(null);
  const workflowGrants = ref<Grant[]>([]);
  const teamMembers = ref<User[]>([]);
  const userTeams = ref<Team[]>([]);
  const userDraft = reactive<UserDraft>(blankUserDraft());
  const apiKeyDraft = reactive<ApiKeyDraft>(blankApiKeyDraft());
  const revealedApiKey = ref<CreateApiKeyResponse | null>(null);
  const teamDraftName = ref("");
  const grantDraft = reactive<GrantDraft>({
    principal_type: "user",
    principal_id: "",
    permission: "view",
  });

  const selectedUser = computed(
    () => users.value.find((user) => user.id === selectedUserId.value) ?? null,
  );
  const selectedTeam = computed(
    () => teams.value.find((team) => team.id === selectedTeamId.value) ?? null,
  );
  const selectedApiKey = computed(
    () => apiKeys.value.find((apiKey) => apiKey.id === selectedApiKeyId.value) ?? null,
  );
  const filteredUsers = computed(() => {
    const query = app.normalizedSearch;

    if (!query) {
      return users.value;
    }

    return users.value.filter((user) => userSearchText(user).includes(query));
  });
  const filteredTeams = computed(() => {
    const query = app.normalizedSearch;

    if (!query) {
      return teams.value;
    }

    return teams.value.filter((team) => teamSearchText(team).includes(query));
  });
  const visibleApiKeys = computed(() => {
    const userId = selectedUserId.value;
    const visible = userId
      ? apiKeys.value.filter((key) => key.is_service || key.user_id === userId)
      : apiKeys.value;
    const query = app.normalizedSearch;

    if (!query) {
      return visible;
    }

    return visible.filter((key) => apiKeySearchText(key, users.value).includes(query));
  });
  const enabledAdminCount = computed(
    () => users.value.filter((user) => user.is_admin && !user.disabled).length,
  );

  async function refreshAll() {
    await app.runOperation("Loading permissions", async () => {
      const [nextUsers, nextTeams, nextApiKeys] = await Promise.all([
        listUsers(),
        listTeams(),
        listApiKeys(),
      ]);
      users.value = nextUsers;
      teams.value = nextTeams;
      apiKeys.value = nextApiKeys;
    });

    if (selectedUserId.value && !selectedUser.value) {
      selectedUserId.value = null;
    }

    if (selectedTeamId.value && !selectedTeam.value) {
      selectedTeamId.value = null;
    }

    if (selectedApiKeyId.value && !selectedApiKey.value) {
      selectedApiKeyId.value = null;
    }

    if (selectedUser.value) {
      await refreshSelectedUserTeams();
    }

    if (selectedTeam.value) {
      await refreshSelectedTeamMembers();
    }
  }

  async function refreshApiKeys() {
    apiKeys.value = await app.runOperation("Loading API keys", () => listApiKeys());

    if (selectedApiKeyId.value && !selectedApiKey.value) {
      clearApiKeyDraft();
    }
  }

  function clearPermissions() {
    users.value = [];
    teams.value = [];
    apiKeys.value = [];
    selectedUserId.value = null;
    selectedTeamId.value = null;
    selectedApiKeyId.value = null;
    selectedWorkflowId.value = null;
    workflowGrants.value = [];
    teamMembers.value = [];
    userTeams.value = [];
    Object.assign(userDraft, blankUserDraft());
    Object.assign(apiKeyDraft, blankApiKeyDraft());
    revealedApiKey.value = null;
    teamDraftName.value = "";
    grantDraft.principal_type = "user";
    grantDraft.principal_id = "";
    grantDraft.permission = "view";
  }

  function selectUser(user: User | null) {
    selectedUserId.value = user?.id ?? null;
    Object.assign(userDraft, user ? userDraftFrom(user) : blankUserDraft());
    userTeams.value = [];

    if (!selectedApiKey.value) {
      Object.assign(apiKeyDraft, blankApiKeyDraft(user?.id ?? null));
    }

    if (user?.id) {
      void refreshSelectedUserTeams();
    }
  }

  function clearUserDraft() {
    selectedUserId.value = null;
    Object.assign(userDraft, blankUserDraft());
    userTeams.value = [];

    if (!selectedApiKey.value) {
      Object.assign(apiKeyDraft, blankApiKeyDraft());
    }
  }

  async function saveUserDraft() {
    const username = userDraft.username.trim();
    const email = userDraft.email.trim();

    if (!selectedUser.value && !username) {
      app.setError("Username is required.");
      return;
    }

    if (!selectedUser.value && !userDraft.password) {
      app.setError("Password is required for new users.");
      return;
    }

    const editing = Boolean(selectedUser.value);

    const saved = await app.runOperation(editing ? "Updating user" : "Creating user", async () => {
      if (selectedUser.value?.id) {
        const request: UpdateUserInput = {
          email: email || null,
          is_admin: userDraft.is_admin,
          disabled: userDraft.disabled,
        };

        if (userDraft.password) {
          request.password = userDraft.password;
        }

        return updateUser(selectedUser.value.id, request);
      }

      const request: CreateUserInput = {
        username,
        password: userDraft.password,
        email: email || null,
        is_admin: userDraft.is_admin,
      };
      return createUser(request);
    });
    await refreshAll();
    selectUser(users.value.find((user) => user.id === saved.id) ?? saved);
    app.setStatus(editing ? "User saved." : "User created.");
  }

  async function deleteSelectedUser() {
    if (!selectedUser.value?.id) {
      return;
    }

    const id = selectedUser.value.id;
    await app.runOperation("Deleting user", () => deleteUser(id));
    clearUserDraft();
    await refreshAll();
    app.setStatus("User deleted.");
  }

  function selectApiKey(apiKey: ApiKey | null) {
    selectedApiKeyId.value = apiKey?.id ?? null;
    Object.assign(
      apiKeyDraft,
      apiKey ? apiKeyDraftFrom(apiKey) : blankApiKeyDraft(selectedUserId.value),
    );
    revealedApiKey.value = null;
  }

  function clearApiKeyDraft() {
    selectedApiKeyId.value = null;
    Object.assign(apiKeyDraft, blankApiKeyDraft(selectedUserId.value));
    revealedApiKey.value = null;
  }

  function clearRevealedApiKey() {
    revealedApiKey.value = null;
  }

  async function saveApiKeyDraft() {
    const name = apiKeyDraft.name.trim();

    if (!name) {
      app.setError("API key name is required.");
      return;
    }

    const editing = Boolean(selectedApiKey.value?.id);
    const saved = await app.runOperation(
      editing ? "Updating API key" : "Creating API key",
      async () => {
        if (selectedApiKey.value?.id) {
          const request: UpdateApiKeyInput = {
            name,
            expires_at: apiKeyDraft.expires_at
              ? new Date(apiKeyDraft.expires_at).toISOString()
              : null,
            disabled: apiKeyDraft.disabled,
          };
          return updateApiKey(selectedApiKey.value.id, request);
        }

        const request: CreateApiKeyInput = {
          name,
          is_service: apiKeyDraft.is_service,
          user_id: apiKeyDraft.is_service ? null : apiKeyDraft.user_id || selectedUserId.value,
          expires_at: apiKeyDraft.expires_at
            ? new Date(apiKeyDraft.expires_at).toISOString()
            : null,
        };
        const created = await createApiKey(request);
        revealedApiKey.value = created;
        return created.api_key;
      },
    );
    await refreshApiKeys();
    const reveal = revealedApiKey.value;
    selectApiKey(apiKeys.value.find((apiKey) => apiKey.id === saved.id) ?? saved);

    if (reveal) {
      revealedApiKey.value = reveal;
    }

    if (!editing && reveal) {
      selectedApiKeyId.value = reveal.api_key.id;
    }

    app.setStatus(editing ? "API key saved." : "API key created.");
  }

  async function revokeSelectedApiKey() {
    const apiKeyId = selectedApiKey.value?.id;

    if (!apiKeyId) {
      return;
    }

    await app.runOperation("Revoking API key", () => revokeApiKey(apiKeyId));
    await refreshApiKeys();

    const refreshed = apiKeys.value.find((apiKey) => apiKey.id === apiKeyId);

    if (refreshed) {
      selectApiKey(refreshed);
    } else {
      clearApiKeyDraft();
    }

    app.setStatus("API key revoked.");
  }

  async function rotateSelectedApiKey() {
    const apiKeyId = selectedApiKey.value?.id;

    if (!apiKeyId) {
      return;
    }

    const rotated = await app.runOperation("Rotating API key", () => rotateApiKey(apiKeyId));
    revealedApiKey.value = rotated;
    await refreshApiKeys();
    selectApiKey(
      apiKeys.value.find((apiKey) => apiKey.id === rotated.api_key.id) ?? rotated.api_key,
    );
    revealedApiKey.value = rotated;
    app.setStatus("API key rotated.");
  }

  async function refreshSelectedUserTeams() {
    const userId = selectedUser.value?.id;

    if (!userId) {
      userTeams.value = [];
      return;
    }

    userTeams.value = await app.runOperation("Loading user teams", () => listUserTeams(userId));
  }

  async function assignSelectedUserToTeam(teamId: string) {
    const userId = selectedUser.value?.id;

    if (!userId || !teamId) {
      return;
    }

    await app.runOperation("Assigning team", () => addTeamMember(teamId, userId));
    await Promise.all([
      refreshSelectedUserTeams(),
      selectedTeam.value ? refreshSelectedTeamMembers() : Promise.resolve(),
    ]);
    app.setStatus("Team assigned.");
  }

  async function removeSelectedUserFromTeam(teamId: string) {
    const userId = selectedUser.value?.id;

    if (!userId || !teamId) {
      return;
    }

    await app.runOperation("Removing team", () => removeTeamMember(teamId, userId));
    await Promise.all([
      refreshSelectedUserTeams(),
      selectedTeam.value ? refreshSelectedTeamMembers() : Promise.resolve(),
    ]);
    app.setStatus("Team removed.");
  }

  function selectTeam(team: Team | null) {
    selectedTeamId.value = team?.id ?? null;
    teamDraftName.value = team?.name ?? "";
    teamMembers.value = [];

    if (team?.id) {
      void refreshSelectedTeamMembers();
    }
  }

  function clearTeamDraft() {
    selectedTeamId.value = null;
    teamDraftName.value = "";
    teamMembers.value = [];
  }

  async function saveTeamDraft() {
    const name = teamDraftName.value.trim();

    if (!name) {
      app.setError("Team name is required.");
      return;
    }

    const editing = Boolean(selectedTeam.value);
    const saved = await app.runOperation(editing ? "Updating team" : "Creating team", () =>
      selectedTeam.value?.id ? updateTeam(selectedTeam.value.id, name) : createTeam(name),
    );
    await refreshAll();
    selectTeam(teams.value.find((team) => team.id === saved.id) ?? saved);
    app.setStatus(editing ? "Team saved." : "Team created.");
  }

  async function deleteSelectedTeam() {
    if (!selectedTeam.value?.id) {
      return;
    }

    const id = selectedTeam.value.id;
    await app.runOperation("Deleting team", () => deleteTeam(id));
    clearTeamDraft();
    await refreshAll();
    app.setStatus("Team deleted.");
  }

  async function refreshSelectedTeamMembers() {
    const teamId = selectedTeam.value?.id;

    if (!teamId) {
      teamMembers.value = [];
      return;
    }

    teamMembers.value = await app.runOperation("Loading team members", () =>
      listTeamMembers(teamId),
    );
  }

  async function addSelectedTeamMember(userId: string) {
    const teamId = selectedTeam.value?.id;

    if (!teamId || !userId) {
      return;
    }

    await app.runOperation("Adding member", () => addTeamMember(teamId, userId));
    await Promise.all([
      refreshSelectedTeamMembers(),
      selectedUser.value ? refreshSelectedUserTeams() : Promise.resolve(),
    ]);
    app.setStatus("Member added.");
  }

  async function removeSelectedTeamMember(userId: string) {
    const teamId = selectedTeam.value?.id;

    if (!teamId || !userId) {
      return;
    }

    await app.runOperation("Removing member", () => removeTeamMember(teamId, userId));
    await Promise.all([
      refreshSelectedTeamMembers(),
      selectedUser.value ? refreshSelectedUserTeams() : Promise.resolve(),
    ]);
    app.setStatus("Member removed.");
  }

  async function selectWorkflow(workflowId: string | null) {
    selectedWorkflowId.value = workflowId;
    workflowGrants.value = [];
    grantDraft.principal_id = "";

    if (workflowId) {
      await refreshWorkflowGrants();
    }
  }

  async function refreshWorkflowGrants() {
    const workflowId = selectedWorkflowId.value;

    if (!workflowId) {
      workflowGrants.value = [];
      return;
    }

    workflowGrants.value = (await app.runOperation("Loading workflow access", () =>
      listWorkflowGrants(workflowId),
    )) as unknown as Grant[];
  }

  async function saveGrantDraft() {
    const workflowId = selectedWorkflowId.value;

    if (!workflowId) {
      app.setError("Select a workflow first.");
      return;
    }

    if (!grantDraft.principal_id) {
      app.setError("Select a user or team first.");
      return;
    }

    await app.runOperation("Saving access", () =>
      grantWorkflowAccess(
        workflowId,
        grantDraft.principal_type,
        grantDraft.principal_id,
        grantDraft.permission,
      ),
    );
    grantDraft.principal_id = "";
    await refreshWorkflowGrants();
    app.setStatus("Access saved.");
  }

  async function revokeGrant(grantId: string | null) {
    const workflowId = selectedWorkflowId.value;

    if (!workflowId || !grantId) {
      return;
    }

    await app.runOperation("Revoking access", () => revokeWorkflowGrant(workflowId, grantId));
    await refreshWorkflowGrants();
    app.setStatus("Access revoked.");
  }

  return {
    users,
    teams,
    apiKeys,
    selectedUserId,
    selectedTeamId,
    selectedApiKeyId,
    selectedWorkflowId,
    workflowGrants,
    teamMembers,
    userTeams,
    userDraft,
    apiKeyDraft,
    revealedApiKey,
    teamDraftName,
    grantDraft,
    selectedUser,
    selectedTeam,
    selectedApiKey,
    filteredUsers,
    filteredTeams,
    visibleApiKeys,
    enabledAdminCount,
    refreshAll,
    refreshApiKeys,
    clearPermissions,
    selectUser,
    clearUserDraft,
    saveUserDraft,
    deleteSelectedUser,
    selectApiKey,
    clearApiKeyDraft,
    clearRevealedApiKey,
    saveApiKeyDraft,
    revokeSelectedApiKey,
    rotateSelectedApiKey,
    refreshSelectedUserTeams,
    assignSelectedUserToTeam,
    removeSelectedUserFromTeam,
    selectTeam,
    clearTeamDraft,
    saveTeamDraft,
    deleteSelectedTeam,
    refreshSelectedTeamMembers,
    addSelectedTeamMember,
    removeSelectedTeamMember,
    selectWorkflow,
    refreshWorkflowGrants,
    saveGrantDraft,
    revokeGrant,
  };
});

function userSearchText(user: User): string {
  return [
    user.id,
    user.username,
    user.email,
    user.is_admin ? "admin" : "user",
    user.disabled ? "disabled" : "enabled",
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function teamSearchText(team: Team): string {
  return [team.id, team.name].filter(Boolean).join(" ").toLowerCase();
}

function apiKeySearchText(apiKey: ApiKey, users: User[]): string {
  const owner = apiKey.user_id
    ? users.find((user) => user.id === apiKey.user_id)?.username
    : "service";
  return [
    apiKey.id,
    apiKey.name,
    apiKey.key_prefix,
    owner,
    apiKey.is_service ? "service" : "user",
    apiKey.disabled ? "disabled" : "active",
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function toDateTimeInput(value: string): string {
  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    return "";
  }

  return date.toISOString().slice(0, 16);
}
