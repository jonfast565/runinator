import { defaultApi, type OrgAdminApi } from "../api/ports/org-admin";
import type { OrgMembership, OrgRole, Organization } from "../api/commandCenterApi";
import type { Team, User } from "../domain/models";
import type { AppService } from "./app";

export function createOrgAdminService(app: AppService, api: OrgAdminApi = defaultApi) {
  return {
    listOrganizations() {
      return app.runOperation("Loading all organizations", () => api.listOrgs());
    },
    deleteOrganization(orgId: string) {
      return app.runOperation("Deleting organization", () => api.deleteOrg(orgId));
    },
    listMembers(orgId: string) {
      return app.runOperation("Loading org members", () => api.listOrgMembers(orgId));
    },
    addMember(orgId: string, userId: string, role: OrgRole) {
      return app.runOperation("Adding org member", () => api.addOrgMember(orgId, userId, role));
    },
    updateMember(orgId: string, userId: string, role: OrgRole) {
      return app.runOperation("Updating org member", () =>
        api.updateOrgMember(orgId, userId, role),
      );
    },
    removeMember(orgId: string, userId: string) {
      return app.runOperation("Removing org member", () => api.removeOrgMember(orgId, userId));
    },
    listUsers() {
      return app.runOperation("Loading users", () => api.listUsers());
    },
    listTeams() {
      return app.runOperation("Loading teams", () => api.listTeams());
    },
    createTeam(name: string) {
      return app.runOperation("Creating team", () => api.createTeam(name));
    },
    deleteTeam(teamId: string) {
      return app.runOperation("Deleting team", () => api.deleteTeam(teamId));
    },
    listTeamMembers(teamId: string) {
      return app.runOperation("Loading team members", () => api.listTeamMembers(teamId));
    },
    addTeamMember(teamId: string, userId: string) {
      return app.runOperation("Adding team member", () =>
        api.addTeamMember(teamId, userId, "member"),
      );
    },
    removeTeamMember(teamId: string, userId: string) {
      return app.runOperation("Removing team member", () => api.removeTeamMember(teamId, userId));
    },
  };
}

export type OrgAdminService = ReturnType<typeof createOrgAdminService>;
export type { OrgMembership, OrgRole, Organization, Team, User };
