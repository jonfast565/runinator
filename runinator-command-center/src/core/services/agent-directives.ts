import { defaultApi, type AgentDirectivesApi } from "../api/ports/agent-directives";

import type { AgentDirectiveKind } from "../domain/models";
import type { AppService } from "./app";

export function createAgentDirectivesService(
  app: AppService,
  api: AgentDirectivesApi = defaultApi,
) {
  return {
    issue(replicaId: string, kind: AgentDirectiveKind) {
      return app.runOperation("Issuing agent directive", () =>
        api.createAgentDirective(replicaId, kind),
      );
    },
    list(replicaId: string) {
      return app.runOperation("Loading agent directives", () => api.listAgentDirectives(replicaId));
    },
    kick(replicaId: string) {
      return app.runOperation("Kicking replica", () => api.kickReplica(replicaId));
    },
  };
}

export type AgentDirectivesService = ReturnType<typeof createAgentDirectivesService>;
