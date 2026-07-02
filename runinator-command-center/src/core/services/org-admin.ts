import {
  addOrgMember,
  addTeamMember,
  createTeam,
  deleteTeam,
  listOrgMembers,
  listTeamMembers,
  listTeams,
  listUsers,
  removeOrgMember,
  removeTeamMember,
  updateOrgMember,
  type OrgMembership,
  type OrgRole,
} from "../api/commandCenterApi";
import type { Team, User } from "../domain/models";
import type { AppService } from "./app";

export function createOrgAdminService(app: AppService) {
  return {
    listMembers(orgId: string) {
      return app.runOperation("Loading org members", () => listOrgMembers(orgId));
    },
    addMember(orgId: string, userId: string, role: OrgRole) {
      return app.runOperation("Adding org member", () => addOrgMember(orgId, userId, role));
    },
    updateMember(orgId: string, userId: string, role: OrgRole) {
      return app.runOperation("Updating org member", () => updateOrgMember(orgId, userId, role));
    },
    removeMember(orgId: string, userId: string) {
      return app.runOperation("Removing org member", () => removeOrgMember(orgId, userId));
    },
    listUsers() {
      return app.runOperation("Loading users", () => listUsers());
    },
    listTeams() {
      return app.runOperation("Loading teams", () => listTeams());
    },
    createTeam(name: string) {
      return app.runOperation("Creating team", () => createTeam(name));
    },
    deleteTeam(teamId: string) {
      return app.runOperation("Deleting team", () => deleteTeam(teamId));
    },
    listTeamMembers(teamId: string) {
      return app.runOperation("Loading team members", () => listTeamMembers(teamId));
    },
    addTeamMember(teamId: string, userId: string) {
      return app.runOperation("Adding team member", () => addTeamMember(teamId, userId));
    },
    removeTeamMember(teamId: string, userId: string) {
      return app.runOperation("Removing team member", () => removeTeamMember(teamId, userId));
    },
  };
}

export type OrgAdminService = ReturnType<typeof createOrgAdminService>;
export type { OrgMembership, OrgRole, Team, User };
