import { defaultApi, type AgentEnrollmentApi } from "../api/ports/agent-enrollment";

import type { CreateAgentEnrollmentTokenInput } from "../domain/models";
import type { AppService } from "./app";

export function createAgentEnrollmentService(
  app: AppService,
  api: AgentEnrollmentApi = defaultApi,
) {
  return {
    create(request: CreateAgentEnrollmentTokenInput) {
      return app.runOperation("Creating enrollment token", () =>
        api.createAgentEnrollmentToken(request),
      );
    },
    list() {
      return app.runOperation("Loading enrollment tokens", () => api.listAgentEnrollmentTokens());
    },
    revoke(tokenId: string) {
      return app.runOperation("Revoking enrollment token", () =>
        api.revokeAgentEnrollmentToken(tokenId),
      );
    },
    machines() {
      return app.runOperation("Loading enrolled machines", () => api.listAgentMachines());
    },
    invalidate(machineId: string) {
      return app.runOperation("Invalidating enrolled machine", () =>
        api.invalidateAgentMachine(machineId),
      );
    },
  };
}

export type AgentEnrollmentService = ReturnType<typeof createAgentEnrollmentService>;
