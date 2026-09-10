import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleInfrastructureApi = Pick<
  typeof Api,
  | "createAgentEnrollmentToken"
  | "createAgentDirective"
  | "createOrg"
  | "fetchNodeBackends"
  | "fetchNodes"
  | "fetchOrgNodes"
  | "fetchOrgUsage"
  | "fetchReplicaProviders"
  | "fetchReplicas"
  | "fetchReplicaSamples"
  | "invalidateAgentMachine"
  | "kickReplica"
  | "listAgentDirectives"
  | "listAgentEnrollmentTokens"
  | "listAgentMachines"
  | "listMyOrgs"
  | "revokeAgentEnrollmentToken"
  | "scaleNodes"
  | "scaleOrgNodes"
  | "stopNode"
>;
