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
} from "../api/commandCenterApi";
import type {
  ApiKey,
  CreateApiKeyResponse,
  Grant,
  PermissionLevel,
  PrincipalType,
  Team,
  User,
} from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

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

export interface PermissionsState {
  users: User[];
  teams: Team[];
  apiKeys: ApiKey[];
  selectedUserId: string | null;
  selectedTeamId: string | null;
  selectedApiKeyId: string | null;
  selectedWorkflowId: string | null;
  workflowGrants: Grant[];
  teamMembers: User[];
  userTeams: Team[];
  revealedApiKey: CreateApiKeyResponse | null;
}

export function blankUserDraft(): UserDraft {
  return {
    username: "",
    email: "",
    password: "",
    is_admin: false,
    disabled: false,
  };
}

export function userDraftFrom(user: User): UserDraft {
  return {
    username: user.username,
    email: user.email ?? "",
    password: "",
    is_admin: user.is_admin,
    disabled: user.disabled,
  };
}

export function blankApiKeyDraft(userId: string | null = null): ApiKeyDraft {
  return {
    name: "",
    user_id: userId ?? "",
    is_service: false,
    expires_at: "",
    disabled: false,
  };
}

export function apiKeyDraftFrom(apiKey: ApiKey): ApiKeyDraft {
  return {
    name: apiKey.name,
    user_id: apiKey.user_id ?? "",
    is_service: apiKey.is_service,
    expires_at: apiKey.expires_at ? toDateTimeInput(apiKey.expires_at) : "",
    disabled: apiKey.disabled,
  };
}

export function blankGrantDraft(): GrantDraft {
  return {
    principal_type: "user",
    principal_id: "",
    permission: "view",
  };
}

export function createPermissionsService(app: AppService) {
  const store = createStore<PermissionsState>({
    users: [],
    teams: [],
    apiKeys: [],
    selectedUserId: null,
    selectedTeamId: null,
    selectedApiKeyId: null,
    selectedWorkflowId: null,
    workflowGrants: [],
    teamMembers: [],
    userTeams: [],
    revealedApiKey: null,
  });

  function selectedUser(): User | null {
    const { users, selectedUserId } = store.getState();
    return users.find((user) => user.id === selectedUserId) ?? null;
  }

  function selectedTeam(): Team | null {
    const { teams, selectedTeamId } = store.getState();
    return teams.find((team) => team.id === selectedTeamId) ?? null;
  }

  function selectedApiKey(): ApiKey | null {
    const { apiKeys, selectedApiKeyId } = store.getState();
    return apiKeys.find((apiKey) => apiKey.id === selectedApiKeyId) ?? null;
  }

  function filteredUsers(query: string): User[] {
    const { users } = store.getState();

    if (!query) {
      return users;
    }

    return users.filter((user) => userSearchText(user).includes(query));
  }

  function filteredTeams(query: string): Team[] {
    const { teams } = store.getState();

    if (!query) {
      return teams;
    }

    return teams.filter((team) => teamSearchText(team).includes(query));
  }

  function visibleApiKeys(query: string): ApiKey[] {
    const { apiKeys, selectedUserId } = store.getState();
    const visible = selectedUserId
      ? apiKeys.filter((key) => key.is_service || key.user_id === selectedUserId)
      : apiKeys;

    if (!query) {
      return visible;
    }

    return visible.filter((key) => apiKeySearchText(key, store.getState().users).includes(query));
  }

  function enabledAdminCount(): number {
    return store.getState().users.filter((user) => user.is_admin && !user.disabled).length;
  }

  const service = {
    ...store,
    selectedUser,
    selectedTeam,
    selectedApiKey,
    filteredUsers,
    filteredTeams,
    visibleApiKeys,
    enabledAdminCount,
    clearRevealedApiKey() {
      store.setState((state) => ({ ...state, revealedApiKey: null }));
    },
    async refreshAll() {
      await app.runOperation("Loading permissions", async () => {
        const [nextUsers, nextTeams, nextApiKeys] = await Promise.all([
          listUsers(),
          listTeams(),
          listApiKeys(),
        ]);
        store.setState((state) => ({
          ...state,
          users: nextUsers,
          teams: nextTeams,
          apiKeys: nextApiKeys,
        }));
      });

      const state = store.getState();

      if (state.selectedUserId && !selectedUser()) {
        store.setState((current) => ({ ...current, selectedUserId: null }));
      }

      if (state.selectedTeamId && !selectedTeam()) {
        store.setState((current) => ({ ...current, selectedTeamId: null }));
      }

      if (state.selectedApiKeyId && !selectedApiKey()) {
        store.setState((current) => ({ ...current, selectedApiKeyId: null }));
      }

      if (selectedUser()) {
        await service.refreshSelectedUserTeams();
      }

      if (selectedTeam()) {
        await service.refreshSelectedTeamMembers();
      }
    },
    async refreshApiKeys() {
      const apiKeys = await app.runOperation("Loading API keys", () => listApiKeys());
      store.setState((state) => ({ ...state, apiKeys }));

      if (store.getState().selectedApiKeyId && !selectedApiKey()) {
        store.setState((state) => ({ ...state, selectedApiKeyId: null, revealedApiKey: null }));
      }
    },
    clearPermissions() {
      store.setState(() => ({
        users: [],
        teams: [],
        apiKeys: [],
        selectedUserId: null,
        selectedTeamId: null,
        selectedApiKeyId: null,
        selectedWorkflowId: null,
        workflowGrants: [],
        teamMembers: [],
        userTeams: [],
        revealedApiKey: null,
      }));
    },
    selectUser(user: User | null) {
      store.setState((state) => ({
        ...state,
        selectedUserId: user?.id ?? null,
        userTeams: user?.id ? state.userTeams : [],
      }));

      if (user?.id) {
        void service.refreshSelectedUserTeams();
      }

      return user ? userDraftFrom(user) : blankUserDraft();
    },
    clearUserSelection() {
      store.setState((state) => ({
        ...state,
        selectedUserId: null,
        userTeams: [],
      }));
      return blankUserDraft();
    },
    async saveUserDraft(userDraft: UserDraft) {
      const username = userDraft.username.trim();
      const email = userDraft.email.trim();
      const currentUser = selectedUser();

      if (!currentUser && !username) {
        app.setError("Username is required.");
        return;
      }

      if (!currentUser && !userDraft.password) {
        app.setError("Password is required for new users.");
        return;
      }

      const editing = Boolean(currentUser);

      const saved = await app.runOperation(editing ? "Updating user" : "Creating user", async () => {
        if (currentUser?.id) {
          const request: UpdateUserInput = {
            email: email || null,
            is_admin: userDraft.is_admin,
            disabled: userDraft.disabled,
          };

          if (userDraft.password) {
            request.password = userDraft.password;
          }

          return updateUser(currentUser.id, request);
        }

        const request: CreateUserInput = {
          username,
          password: userDraft.password,
          email: email || null,
          is_admin: userDraft.is_admin,
        };
        return createUser(request);
      });
      await service.refreshAll();
      service.selectUser(store.getState().users.find((user) => user.id === saved.id) ?? saved);
      app.setStatus(editing ? "User saved." : "User created.");
    },
    async deleteSelectedUser() {
      const user = selectedUser();

      if (!user?.id) {
        return;
      }

      const userId = user.id;
      await app.runOperation("Deleting user", () => deleteUser(userId));
      service.clearUserSelection();
      await service.refreshAll();
      app.setStatus("User deleted.");
    },
    selectApiKey(apiKey: ApiKey | null, selectedUserId: string | null) {
      store.setState((state) => ({
        ...state,
        selectedApiKeyId: apiKey?.id ?? null,
        revealedApiKey: null,
      }));
      return apiKey ? apiKeyDraftFrom(apiKey) : blankApiKeyDraft(selectedUserId);
    },
    clearApiKeySelection(selectedUserId: string | null) {
      store.setState((state) => ({
        ...state,
        selectedApiKeyId: null,
        revealedApiKey: null,
      }));
      return blankApiKeyDraft(selectedUserId);
    },
    async saveApiKeyDraft(apiKeyDraft: ApiKeyDraft) {
      const name = apiKeyDraft.name.trim();

      if (!name) {
        app.setError("API key name is required.");
        return;
      }

      const currentApiKey = selectedApiKey();
      const editing = Boolean(currentApiKey?.id);
      const selectedUserId = store.getState().selectedUserId;

      const saved = await app.runOperation(
        editing ? "Updating API key" : "Creating API key",
        async () => {
          if (currentApiKey?.id) {
            const request: UpdateApiKeyInput = {
              name,
              expires_at: apiKeyDraft.expires_at
                ? new Date(apiKeyDraft.expires_at).toISOString()
                : null,
              disabled: apiKeyDraft.disabled,
            };
            return updateApiKey(currentApiKey.id, request);
          }

          const request: CreateApiKeyInput = {
            name,
            is_service: apiKeyDraft.is_service,
            user_id: apiKeyDraft.is_service ? null : apiKeyDraft.user_id || selectedUserId,
            expires_at: apiKeyDraft.expires_at
              ? new Date(apiKeyDraft.expires_at).toISOString()
              : null,
          };
          const created = await createApiKey(request);
          store.setState((state) => ({ ...state, revealedApiKey: created }));
          return created.api_key;
        },
      );
      await service.refreshApiKeys();
      const reveal = store.getState().revealedApiKey;
      service.selectApiKey(
        store.getState().apiKeys.find((apiKey) => apiKey.id === saved.id) ?? saved,
        selectedUserId,
      );

      if (reveal) {
        store.setState((state) => ({ ...state, revealedApiKey: reveal }));
      }

      if (!editing && reveal) {
        store.setState((state) => ({ ...state, selectedApiKeyId: reveal.api_key.id }));
      }

      app.setStatus(editing ? "API key saved." : "API key created.");
    },
    async revokeSelectedApiKey() {
      const apiKeyId = selectedApiKey()?.id;

      if (!apiKeyId) {
        return;
      }

      await app.runOperation("Revoking API key", () => revokeApiKey(apiKeyId));
      await service.refreshApiKeys();

      const refreshed = store.getState().apiKeys.find((apiKey) => apiKey.id === apiKeyId);

      if (refreshed) {
        service.selectApiKey(refreshed, store.getState().selectedUserId);
      } else {
        service.clearApiKeySelection(store.getState().selectedUserId);
      }

      app.setStatus("API key revoked.");
    },
    async rotateSelectedApiKey() {
      const apiKeyId = selectedApiKey()?.id;

      if (!apiKeyId) {
        return;
      }

      const rotated = await app.runOperation("Rotating API key", () => rotateApiKey(apiKeyId));
      store.setState((state) => ({ ...state, revealedApiKey: rotated }));
      await service.refreshApiKeys();
      service.selectApiKey(
        store.getState().apiKeys.find((apiKey) => apiKey.id === rotated.api_key.id) ??
          rotated.api_key,
        store.getState().selectedUserId,
      );
      store.setState((state) => ({ ...state, revealedApiKey: rotated }));
      app.setStatus("API key rotated.");
    },
    async refreshSelectedUserTeams() {
      const userId = selectedUser()?.id;

      if (!userId) {
        store.setState((state) => ({ ...state, userTeams: [] }));
        return;
      }

      const userTeams = await app.runOperation("Loading user teams", () => listUserTeams(userId));
      store.setState((state) => ({ ...state, userTeams }));
    },
    async assignSelectedUserToTeam(teamId: string) {
      const userId = selectedUser()?.id;

      if (!userId || !teamId) {
        return;
      }

      await app.runOperation("Assigning team", () => addTeamMember(teamId, userId));
      await Promise.all([
        service.refreshSelectedUserTeams(),
        selectedTeam() ? service.refreshSelectedTeamMembers() : Promise.resolve(),
      ]);
      app.setStatus("Team assigned.");
    },
    async removeSelectedUserFromTeam(teamId: string) {
      const userId = selectedUser()?.id;

      if (!userId || !teamId) {
        return;
      }

      await app.runOperation("Removing team", () => removeTeamMember(teamId, userId));
      await Promise.all([
        service.refreshSelectedUserTeams(),
        selectedTeam() ? service.refreshSelectedTeamMembers() : Promise.resolve(),
      ]);
      app.setStatus("Team removed.");
    },
    selectTeam(team: Team | null) {
      store.setState((state) => ({
        ...state,
        selectedTeamId: team?.id ?? null,
        teamMembers: team?.id ? state.teamMembers : [],
      }));

      if (team?.id) {
        void service.refreshSelectedTeamMembers();
      }

      return team?.name ?? "";
    },
    clearTeamSelection() {
      store.setState((state) => ({
        ...state,
        selectedTeamId: null,
        teamMembers: [],
      }));
      return "";
    },
    async saveTeamDraft(teamDraftName: string) {
      const name = teamDraftName.trim();

      if (!name) {
        app.setError("Team name is required.");
        return;
      }

      const currentTeam = selectedTeam();
      const editing = Boolean(currentTeam);
      const saved = await app.runOperation(editing ? "Updating team" : "Creating team", () =>
        currentTeam?.id ? updateTeam(currentTeam.id, name) : createTeam(name),
      );
      await service.refreshAll();
      service.selectTeam(store.getState().teams.find((team) => team.id === saved.id) ?? saved);
      app.setStatus(editing ? "Team saved." : "Team created.");
    },
    async deleteSelectedTeam() {
      const team = selectedTeam();

      if (!team?.id) {
        return;
      }

      const teamId = team.id;
      await app.runOperation("Deleting team", () => deleteTeam(teamId));
      service.clearTeamSelection();
      await service.refreshAll();
      app.setStatus("Team deleted.");
    },
    async refreshSelectedTeamMembers() {
      const teamId = selectedTeam()?.id;

      if (!teamId) {
        store.setState((state) => ({ ...state, teamMembers: [] }));
        return;
      }

      const teamMembers = await app.runOperation("Loading team members", () =>
        listTeamMembers(teamId),
      );
      store.setState((state) => ({ ...state, teamMembers }));
    },
    async addSelectedTeamMember(userId: string) {
      const teamId = selectedTeam()?.id;

      if (!teamId || !userId) {
        return;
      }

      await app.runOperation("Adding member", () => addTeamMember(teamId, userId));
      await Promise.all([
        service.refreshSelectedTeamMembers(),
        selectedUser() ? service.refreshSelectedUserTeams() : Promise.resolve(),
      ]);
      app.setStatus("Member added.");
    },
    async removeSelectedTeamMember(userId: string) {
      const teamId = selectedTeam()?.id;

      if (!teamId || !userId) {
        return;
      }

      await app.runOperation("Removing member", () => removeTeamMember(teamId, userId));
      await Promise.all([
        service.refreshSelectedTeamMembers(),
        selectedUser() ? service.refreshSelectedUserTeams() : Promise.resolve(),
      ]);
      app.setStatus("Member removed.");
    },
    async selectWorkflow(workflowId: string | null) {
      store.setState((state) => ({
        ...state,
        selectedWorkflowId: workflowId,
        workflowGrants: workflowId ? state.workflowGrants : [],
      }));

      if (workflowId) {
        await service.refreshWorkflowGrants();
      }

      return blankGrantDraft();
    },
    async refreshWorkflowGrants() {
      const workflowId = store.getState().selectedWorkflowId;

      if (!workflowId) {
        store.setState((state) => ({ ...state, workflowGrants: [] }));
        return;
      }

      const workflowGrants = (await app.runOperation("Loading workflow access", () =>
        listWorkflowGrants(workflowId),
      )) as unknown as Grant[];
      store.setState((state) => ({ ...state, workflowGrants }));
    },
    async saveGrantDraft(grantDraft: GrantDraft) {
      const workflowId = store.getState().selectedWorkflowId;

      if (!workflowId) {
        app.setError("Select a workflow first.");
        return blankGrantDraft();
      }

      if (!grantDraft.principal_id) {
        app.setError("Select a user or team first.");
        return grantDraft;
      }

      await app.runOperation("Saving access", () =>
        grantWorkflowAccess(
          workflowId,
          grantDraft.principal_type,
          grantDraft.principal_id,
          grantDraft.permission,
        ),
      );
      await service.refreshWorkflowGrants();
      app.setStatus("Access saved.");
      return blankGrantDraft();
    },
    async revokeGrant(grantId: string | null) {
      const workflowId = store.getState().selectedWorkflowId;

      if (!workflowId || !grantId) {
        return;
      }

      await app.runOperation("Revoking access", () => revokeWorkflowGrant(workflowId, grantId));
      await service.refreshWorkflowGrants();
      app.setStatus("Access revoked.");
    },
  };

  return service;
}

export type PermissionsService = ReturnType<typeof createPermissionsService>;

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
