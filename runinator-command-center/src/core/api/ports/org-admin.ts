import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type OrgAdminApi = Pick<
  typeof Api,
  | "addOrgMember"
  | "addTeamMember"
  | "createTeam"
  | "deleteOrg"
  | "deleteTeam"
  | "listOrgs"
  | "listOrgMembers"
  | "listTeamMembers"
  | "listTeams"
  | "listUsers"
  | "removeOrgMember"
  | "removeTeamMember"
  | "updateOrgMember"
>;
