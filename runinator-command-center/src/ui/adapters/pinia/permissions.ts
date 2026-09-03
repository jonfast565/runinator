import { defineStore } from "pinia";
import { computed, reactive } from "vue";
import {
  blankApiKeyDraft,
  blankGrantDraft,
  blankUserDraft,
  permissionLevels,
  type ApiKeyDraft,
  type GrantDraft,
  type UserDraft,
} from "../../../core/services/permissions";
import { appService, permissionsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";
import type { ApiKey, Team, User } from "../../../core/domain/models";

export { permissionLevels };
export type { UserDraft, GrantDraft, ApiKeyDraft };

export const usePermissionsStore = defineStore("permissions", () => {
  const state = mirrorServiceState(permissionsService);
  const userDraft = reactive<UserDraft>(blankUserDraft());
  const apiKeyDraft = reactive<ApiKeyDraft>(blankApiKeyDraft());
  const grantDraft = reactive<GrantDraft>(blankGrantDraft());
  const teamDraftName = reactive({ value: "" });

  return {
    users: computed(() => state.value.users),
    teams: computed(() => state.value.teams),
    apiKeys: computed(() => state.value.apiKeys),
    selectedUserId: computed(() => state.value.selectedUserId),
    selectedTeamId: computed(() => state.value.selectedTeamId),
    selectedApiKeyId: computed(() => state.value.selectedApiKeyId),
    selectedResourceType: computed(() => state.value.selectedResourceType),
    selectedResourceId: computed(() => state.value.selectedResourceId),
    accessResources: computed(() => state.value.accessResources),
    resourceGrants: computed(() => state.value.resourceGrants),
    teamMembers: computed(() => state.value.teamMembers),
    userTeams: computed(() => state.value.userTeams),
    userDraft,
    apiKeyDraft,
    revealedApiKey: computed(() => state.value.revealedApiKey),
    teamDraftName: computed({
      get: () => teamDraftName.value,
      set: (value: string) => {
        teamDraftName.value = value;
      },
    }),
    grantDraft,
    selectedUser: computed(() => permissionsService.selectedUser()),
    selectedTeam: computed(() => permissionsService.selectedTeam()),
    selectedApiKey: computed(() => permissionsService.selectedApiKey()),
    filteredUsers: computed(() => permissionsService.filteredUsers(appService.normalizedSearch)),
    filteredTeams: computed(() => permissionsService.filteredTeams(appService.normalizedSearch)),
    visibleApiKeys: computed(() => permissionsService.visibleApiKeys(appService.normalizedSearch)),
    enabledAdminCount: computed(() => permissionsService.enabledAdminCount()),
    refreshAll: () => permissionsService.refreshAll(),
    refreshApiKeys: () => permissionsService.refreshApiKeys(),
    clearPermissions: () => {
      permissionsService.clearPermissions();
      Object.assign(userDraft, blankUserDraft());
      Object.assign(apiKeyDraft, blankApiKeyDraft());
      Object.assign(grantDraft, blankGrantDraft());
      teamDraftName.value = "";
    },
    selectUser: (user: User | null) => {
      Object.assign(userDraft, permissionsService.selectUser(user));

      if (!permissionsService.selectedApiKey()) {
        Object.assign(apiKeyDraft, blankApiKeyDraft(user?.id ?? null));
      }
    },
    clearUserDraft: () => {
      Object.assign(userDraft, permissionsService.clearUserSelection());

      if (!permissionsService.selectedApiKey()) {
        Object.assign(apiKeyDraft, blankApiKeyDraft());
      }
    },
    saveUserDraft: () => permissionsService.saveUserDraft({ ...userDraft }),
    deleteSelectedUser: () => permissionsService.deleteSelectedUser(),
    selectApiKey: (apiKey: ApiKey | null) => {
      Object.assign(
        apiKeyDraft,
        permissionsService.selectApiKey(apiKey, state.value.selectedUserId),
      );
    },
    clearApiKeyDraft: () => {
      Object.assign(
        apiKeyDraft,
        permissionsService.clearApiKeySelection(state.value.selectedUserId),
      );
    },
    clearRevealedApiKey: () => { permissionsService.clearRevealedApiKey(); },
    saveApiKeyDraft: () => permissionsService.saveApiKeyDraft({ ...apiKeyDraft }),
    revokeSelectedApiKey: () => permissionsService.revokeSelectedApiKey(),
    rotateSelectedApiKey: () => permissionsService.rotateSelectedApiKey(),
    refreshSelectedUserTeams: () => permissionsService.refreshSelectedUserTeams(),
    assignSelectedUserToTeam: (teamId: string) =>
      permissionsService.assignSelectedUserToTeam(teamId),
    removeSelectedUserFromTeam: (teamId: string) =>
      permissionsService.removeSelectedUserFromTeam(teamId),
    selectTeam: (team: Team | null) => {
      teamDraftName.value = permissionsService.selectTeam(team);
    },
    clearTeamDraft: () => {
      teamDraftName.value = permissionsService.clearTeamSelection();
    },
    saveTeamDraft: () => permissionsService.saveTeamDraft(teamDraftName.value),
    deleteSelectedTeam: () => permissionsService.deleteSelectedTeam(),
    refreshSelectedTeamMembers: () => permissionsService.refreshSelectedTeamMembers(),
    addSelectedTeamMember: (userId: string) => permissionsService.addSelectedTeamMember(userId),
    removeSelectedTeamMember: (userId: string) =>
      permissionsService.removeSelectedTeamMember(userId),
    refreshAccessResources: (resourceType?: import("../../../core/services/permissions").AccessResourceType) =>
      permissionsService.refreshAccessResources(resourceType),
    selectResource: async (resourceId: string | null) => {
      Object.assign(grantDraft, await permissionsService.selectResource(resourceId));
    },
    refreshResourceGrants: () => permissionsService.refreshResourceGrants(),
    saveResourceGrant: async () => {
      Object.assign(grantDraft, await permissionsService.saveResourceGrant({ ...grantDraft }));
    },
    revokeSelectedResourceGrant: (grantId: string | null) =>
      permissionsService.revokeSelectedResourceGrant(grantId),
  };
});
