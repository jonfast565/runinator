import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AgentEnrollmentApi = Pick<
  typeof Api,
  | "createAgentEnrollmentToken"
  | "invalidateAgentMachine"
  | "listAgentMachines"
  | "listAgentEnrollmentTokens"
  | "revokeAgentEnrollmentToken"
>;
