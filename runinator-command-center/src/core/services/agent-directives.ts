import { createAgentDirective, listAgentDirectives } from "../api/commandCenterApi";
import type { AgentDirectiveKind } from "../domain/models";
import type { AppService } from "./app";

export function createAgentDirectivesService(app: AppService) {
  return {
    issue(replicaId: string, kind: AgentDirectiveKind) {
      return app.runOperation("Issuing agent directive", () => createAgentDirective(replicaId, kind));
    },
    list(replicaId: string) {
      return app.runOperation("Loading agent directives", () => listAgentDirectives(replicaId));
    },
  };
}

export type AgentDirectivesService = ReturnType<typeof createAgentDirectivesService>;
