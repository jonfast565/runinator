import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AgentDirectivesApi = Pick<
  typeof Api,
  "createAgentDirective" | "kickReplica" | "listAgentDirectives"
>;
